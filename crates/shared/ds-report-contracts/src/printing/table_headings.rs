//! Table heading modes: whether a printed table keeps literal titles or
//! aliases wide ones to letters with a key above every panel. The plan that
//! measures the titles and assigns the aliases is printing computation and
//! lives in the Reporter.
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
/// A full pole workbook can carry fourteen fields. Bound at one alphabet,
/// rather than dropping workbook columns to satisfy the old twelve-field cap.
pub const MAX_COLUMNS: usize = 26;

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
