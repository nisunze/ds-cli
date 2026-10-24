//! The value-key setup of a print table: which columns may print their values
//! as number indexes with a legend under the panel (`Category: 1 Residential
//! · 2 Commercial`), and how many categories a compacted column may hold.
//!
//! This is the document and its admission only. The plan that measures the
//! rows and decides which columns compact, which index each value takes and
//! what the legend says is printing computation and lives in the Reporter.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Columns with up to this many categories compact unless the setup says otherwise.
pub const DEFAULT_MAX_CATEGORIES: usize = 3;
/// A legend of one entry is a caption, not a key; two is the smallest key.
pub const MIN_MAX_CATEGORIES: usize = 2;
/// One alphabet: the heading key's own ceiling, kept for the values.
pub const MAX_MAX_CATEGORIES: usize = super::table_headings::MAX_COLUMNS;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValueKeyMode {
    /// Every value prints literally.
    #[default]
    Off,
    /// Every bound column whose non-empty values recur in two to
    /// `max_categories` categories, and whose widest value is wider than its
    /// index, compacts.
    Auto,
    /// Exactly the listed columns compact; one with more categories than
    /// `max_categories` allows refuses, naming the column and its count.
    Columns,
}

/// A table's value-key setup. Absent on a binding, the table's own default
/// applies: `auto` for the customers (house connection) schedule, `off` for
/// every other table.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ValueKey {
    #[serde(default)]
    pub mode: ValueKeyMode,
    /// Most distinct values a compacted column may hold, 2 to 26.
    #[serde(default = "default_max_categories")]
    pub max_categories: usize,
    /// Bound column keys; read in `columns` mode only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
}
fn default_max_categories() -> usize {
    DEFAULT_MAX_CATEGORIES
}
impl Default for ValueKey {
    fn default() -> Self {
        Self::off()
    }
}
impl ValueKey {
    pub const fn off() -> Self {
        Self {
            mode: ValueKeyMode::Off,
            max_categories: DEFAULT_MAX_CATEGORIES,
            columns: Vec::new(),
        }
    }
    pub const fn auto() -> Self {
        Self {
            mode: ValueKeyMode::Auto,
            max_categories: DEFAULT_MAX_CATEGORIES,
            columns: Vec::new(),
        }
    }
    pub fn is_off(&self) -> bool {
        self.mode == ValueKeyMode::Off
    }
    /// The table's own default when a binding never chose: the customers
    /// (house connection) schedule compacts, every other table prints literally.
    pub fn table_default(binding: &super::TableBinding) -> Self {
        let customers = match &binding.workbook {
            Some(workbook) => workbook.sheet == super::WorkbookSheet::Customers,
            None => binding.layer == "customers",
        };
        if customers {
            Self::auto()
        } else {
            Self::off()
        }
    }
    /// `bound` is the table's column list when the layout knows it; a workbook
    /// binding resolves its columns at render time and checks them there.
    pub fn validate(&self, bound: Option<&[String]>) -> Result<(), String> {
        if !(MIN_MAX_CATEGORIES..=MAX_MAX_CATEGORIES).contains(&self.max_categories) {
            return Err(format!(
                "value_key max_categories must be between {MIN_MAX_CATEGORIES} and {MAX_MAX_CATEGORIES}"
            ));
        }
        if self.columns.len() > super::table_headings::MAX_COLUMNS {
            return Err("value_key names more columns than a table can bind".into());
        }
        if self.mode == ValueKeyMode::Columns && self.columns.is_empty() {
            return Err("value_key columns mode names at least one bound column".into());
        }
        for (i, key) in self.columns.iter().enumerate() {
            if key.is_empty() || key.len() > 160 || self.columns[..i].contains(key) {
                return Err(format!("value_key column `{key}` is empty or repeated"));
            }
            if bound.is_some_and(|bound| !bound.contains(key)) {
                return Err(format!("value_key column `{key}` is not a bound column"));
            }
        }
        Ok(())
    }
    /// Keep only the columns the table still binds; a column selection that
    /// drops a compacted column drops it here too instead of refusing later.
    pub fn retain_bound(&mut self, bound: &[String]) {
        self.columns.retain(|key| bound.contains(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_column_selection_drops_a_compacted_column_it_no_longer_binds() {
        let mut key = ValueKey {
            mode: ValueKeyMode::Columns,
            max_categories: 3,
            columns: vec!["category".into(), "meter_type".into()],
        };
        key.retain_bound(&["category".into()]);
        assert_eq!(key.columns, ["category"]);
    }
    #[test]
    fn the_customers_schedule_compacts_by_default_and_the_poles_schedule_does_not() {
        use super::super::{TableBinding, WorkbookSheet, WorkbookTableBinding};
        let customers = TableBinding {
            layer: "customers".into(),
            ..Default::default()
        };
        assert_eq!(ValueKey::table_default(&customers), ValueKey::auto());
        assert_eq!(customers.value_key(), ValueKey::auto());
        let workbook = TableBinding {
            workbook: Some(WorkbookTableBinding {
                sheet: WorkbookSheet::Customers,
                as_built: true,
            }),
            ..Default::default()
        };
        assert_eq!(workbook.value_key(), ValueKey::auto());
        let poles = TableBinding {
            layer: "lv_poles".into(),
            ..Default::default()
        };
        assert_eq!(poles.value_key(), ValueKey::off());
        let workbook_poles = TableBinding {
            workbook: Some(WorkbookTableBinding {
                sheet: WorkbookSheet::Poles,
                as_built: false,
            }),
            ..Default::default()
        };
        assert_eq!(workbook_poles.value_key(), ValueKey::off());
        // An explicit choice wins over the table's default, both ways.
        let quiet = TableBinding {
            value_key: Some(ValueKey::off()),
            ..customers
        };
        assert_eq!(quiet.value_key(), ValueKey::off());
        let loud = TableBinding {
            value_key: Some(ValueKey::auto()),
            ..poles
        };
        assert_eq!(loud.value_key(), ValueKey::auto());
    }
}

#[cfg(test)]
mod document_tests {
    use super::*;
    use crate::printing::{overrides::TableOverride, TableBinding};
    use serde_json::json;
    #[test]
    fn a_document_that_never_chose_stays_free_of_the_field_and_a_choice_round_trips() {
        let old = json!({"layer":"customers","columns":["category"],"headings":["Category"],"widths":[1]});
        let mut table: TableBinding = serde_json::from_value(old.clone()).unwrap();
        assert!(serde_json::to_value(&table)
            .unwrap()
            .get("value_key")
            .is_none());
        assert_eq!(table.value_key, None);
        table.value_key = Some(ValueKey {
            mode: ValueKeyMode::Columns,
            max_categories: 4,
            columns: vec!["category".into()],
        });
        let saved = serde_json::to_value(&table).unwrap();
        assert_eq!(
            saved["value_key"],
            json!({"mode":"columns","max_categories":4,"columns":["category"]})
        );
        assert_eq!(
            serde_json::from_value::<TableBinding>(saved).unwrap(),
            table
        );
        // A bare mode reads with the default ceiling; unknown modes and
        // unknown fields are closed.
        let bare: TableBinding =
            serde_json::from_value(json!({"layer":"customers","columns":["category"],"headings":["Category"],"widths":[1],"value_key":{"mode":"off"}})).unwrap();
        assert_eq!(bare.value_key, Some(ValueKey::off()));
        let mut bad = old.clone();
        bad["value_key"] = json!({"mode":"legend"});
        assert!(serde_json::from_value::<TableBinding>(bad).is_err());
        let mut bad = old;
        bad["value_key"] = json!({"mode":"auto","legend":"bottom"});
        assert!(serde_json::from_value::<TableBinding>(bad).is_err());
        let patch: TableOverride =
            serde_json::from_value(json!({"value_key":{"mode":"auto","max_categories":5}}))
                .unwrap();
        assert_eq!(
            patch.value_key,
            Some(ValueKey {
                mode: ValueKeyMode::Auto,
                max_categories: 5,
                columns: vec![]
            })
        );
        assert!(serde_json::to_value(TableOverride::default())
            .unwrap()
            .get("value_key")
            .is_none());
    }
    #[test]
    fn the_layout_refuses_a_value_key_naming_an_unbound_column_or_a_bad_ceiling() {
        let mut layout = crate::printing::default_layout();
        let mut element = crate::printing::elements::new_element(
            crate::printing::ElementKind::Table,
            "customers".into(),
            "",
        );
        let table = element.table.as_mut().unwrap();
        table.layer = "customers".into();
        table.columns = vec!["house_number".into(), "category".into()];
        table.headings = vec!["House".into(), "Category".into()];
        table.widths = vec![1., 1.];
        table.value_key = Some(ValueKey {
            mode: ValueKeyMode::Columns,
            max_categories: 3,
            columns: vec!["village".into()],
        });
        layout.elements.push(element);
        let refused = crate::printing::validate(&layout).unwrap_err();
        assert!(refused.contains("`village`"), "{refused}");
        let table = layout.elements.last_mut().unwrap().table.as_mut().unwrap();
        table.value_key.as_mut().unwrap().columns = vec!["category".into()];
        crate::printing::validate(&layout).unwrap();
        let table = layout.elements.last_mut().unwrap().table.as_mut().unwrap();
        table.value_key.as_mut().unwrap().max_categories = 1;
        assert!(crate::printing::validate(&layout)
            .unwrap_err()
            .contains("max_categories"));
        // A transformer override carries the same field through the same check.
        let table = layout.elements.last_mut().unwrap().table.as_mut().unwrap();
        table.value_key = None;
        crate::printing::validate(&layout).unwrap();
        let patch: crate::printing::overrides::PrintOverride = serde_json::from_value(
            json!({"elements":{"customers":{"table":{"value_key":{"mode":"columns","columns":["category"]}}}}}),
        )
        .unwrap();
        let changed = crate::printing::overrides::apply(&layout, &patch).unwrap();
        assert_eq!(
            changed
                .elements
                .last()
                .unwrap()
                .table
                .as_ref()
                .unwrap()
                .value_key,
            Some(ValueKey {
                mode: ValueKeyMode::Columns,
                max_categories: 3,
                columns: vec!["category".into()]
            })
        );
        let patch: crate::printing::overrides::PrintOverride = serde_json::from_value(
            json!({"elements":{"customers":{"table":{"value_key":{"mode":"columns","columns":["nid"]}}}}}),
        )
        .unwrap();
        assert!(crate::printing::overrides::apply(&layout, &patch)
            .unwrap_err()
            .contains("`nid`"));
    }
}
