//! Table heading identity is independent of renderer font measurement.
//! Renderers supply measured widths; the kernel chooses stable ordered aliases.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeadingMode {
    Literal,
    /// Default: fitting titles stay literal; only wider titles receive
    /// compact aliases and a full definition in the key above every panel.
    #[default]
    AutoIndex,
    Indexed,
    /// Native typography may abbreviate a long title, retaining its full key;
    /// stable letters remain the fallback when abbreviations collide or cannot fit.
    Abbreviated,
}
impl HeadingMode {
    pub fn is_literal(&self) -> bool {
        *self == Self::Literal
    }
    /// Serialization skips the default, so a document that never chose a mode
    /// stays free of the field.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
#[derive(Debug, PartialEq)]
pub struct HeadingPlan {
    pub headings: Vec<String>,
    pub key: Vec<(String, String)>,
}
/// A full pole workbook can carry fourteen fields. Bound at one alphabet,
/// rather than dropping workbook columns to satisfy the old twelve-field cap.
pub const MAX_COLUMNS: usize = 26;
/// Wider headings receive deterministic aliases; literal letter titles are reserved.
pub fn plan(
    mode: HeadingMode,
    headings: &[String],
    heading_widths: &[f64],
    data_widths: &[f64],
) -> Result<HeadingPlan, String> {
    let count = headings.len();
    if count == 0
        || count > MAX_COLUMNS
        || heading_widths.len() != count
        || data_widths.len() != count
        || heading_widths
            .iter()
            .chain(data_widths)
            .any(|w| !w.is_finite() || *w < 0.)
    {
        return Err("Invalid table heading measurements.".into());
    }
    let indexed: Vec<_> = heading_widths
        .iter()
        .zip(data_widths)
        .map(|(h, d)| match mode {
            HeadingMode::Literal => false,
            HeadingMode::Indexed => true,
            HeadingMode::AutoIndex | HeadingMode::Abbreviated => *h > *d + 1e-7,
        })
        .collect();
    if headings
        .iter()
        .zip(&indexed)
        .any(|(heading, indexed)| *indexed && heading.trim().is_empty())
    {
        return Err("Indexed table headings require a full label for every aliased column.".into());
    }
    // Reserve fitting literal letter titles (including x/y) so aliases never
    // ambiguously share their visible identity with another column.
    let reserved: std::collections::BTreeSet<_> = headings
        .iter()
        .zip(&indexed)
        .filter(|(_, indexed)| !**indexed)
        .map(|(heading, _)| heading.trim().to_uppercase())
        .collect();
    let mut letters = (b'A'..=b'Z')
        .map(|letter| (letter as char).to_string())
        .filter(|letter| !reserved.contains(letter));
    let mut result = HeadingPlan {
        headings: Vec::with_capacity(count),
        key: vec![],
    };
    for (heading, indexed) in headings.iter().zip(indexed) {
        if indexed {
            let alias = letters.next().ok_or("Table heading aliases exhausted.")?;
            result.key.push((alias.clone(), heading.clone()));
            result.headings.push(alias);
        } else {
            result.headings.push(heading.clone());
        }
    }
    Ok(result)
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
        assert_eq!(indexed.headings, ["B", "A", "C", "D"]);
        assert_eq!(indexed.key[0], ("B".into(), "House Number".into()));
        assert_eq!(indexed.key[2], ("D".into(), "Earthing".into()));
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
    fn default_keeps_description_and_coordinates_literal_and_keys_long_titles() {
        let labels = ["Number", "Description", "Flying stay", "X", "Y"].map(str::to_owned);
        let result = plan(
            HeadingMode::default(),
            &labels,
            &[12., 20., 18., 2., 2.],
            &[4., 35., 4., 15., 15.],
        )
        .unwrap();
        assert_eq!(result.headings, ["A", "Description", "B", "X", "Y"]);
        assert_eq!(
            result.key,
            [
                ("A".into(), "Number".into()),
                ("B".into(), "Flying stay".into())
            ]
        );
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
        // auto_index is the default, so an old document that never chose a
        // mode still serializes without the field — and now reads back as
        // "a heading never widens its column".
        assert_eq!(table.heading_mode, HeadingMode::AutoIndex);
        // An explicitly chosen NON-default mode is written, so a layout that
        // deliberately wants literal headings keeps them across a round trip.
        table.heading_mode = HeadingMode::Literal;
        let saved = serde_json::to_value(&table).unwrap();
        assert_eq!(saved["heading_mode"], "literal");
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
