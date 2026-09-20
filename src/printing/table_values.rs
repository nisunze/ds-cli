//! Column values compacted to number indexes, with a legend under the panel.
//!
//! The heading key ([`super::table_headings`]) aliases TITLES and prints their
//! key above every panel. This is its VALUES counterpart: a column whose
//! values fall into a few categories (`Residential`, `Commercial`) prints
//! `1`/`2` in every row, and one legend line below the panel names them
//! (`Category: 1 Residential · 2 Commercial`). The legend always carries the
//! FULL heading, never the heading key's alias, so a reader needs one lookup.
//!
//! Renderers supply measurement; the kernel decides which columns compact,
//! which index each value takes and what the legend says. Indexes are
//! 1-based and follow the values sorted case-insensitively, so the same rows
//! print the same legend on every host.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Columns with up to this many categories compact unless the setup says otherwise.
pub const DEFAULT_MAX_CATEGORIES: usize = 3;
/// A legend of one entry is a caption, not a key; two is the smallest key.
pub const MIN_MAX_CATEGORIES: usize = 2;
/// One alphabet: the heading key's own ceiling, kept for the values.
pub const MAX_MAX_CATEGORIES: usize = super::table_headings::MAX_COLUMNS;
/// Between legend entries: `1 Residential · 2 Commercial`.
pub const LEGEND_SEPARATOR: &str = " · ";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactedColumn {
    /// Position among the bound columns.
    pub column: usize,
    /// The full heading the legend names, never the heading key's alias.
    pub heading: String,
    /// Distinct non-empty values in index order; a value's index is its position plus one.
    pub values: Vec<String>,
}
impl CompactedColumn {
    /// The index a literal prints as; an empty literal prints empty.
    pub fn index(&self, literal: &str) -> String {
        let literal = literal.trim();
        if literal.is_empty() {
            return String::new();
        }
        self.values
            .iter()
            .position(|v| v == literal)
            .map(|i| (i + 1).to_string())
            .unwrap_or_else(|| literal.to_owned())
    }
    pub fn legend_line(&self) -> String {
        format!(
            "{}: {}",
            self.heading,
            self.values
                .iter()
                .enumerate()
                .map(|(i, v)| format!("{} {v}", i + 1))
                .collect::<Vec<_>>()
                .join(LEGEND_SEPARATOR)
        )
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValuePlan {
    /// Compacted columns in bound column order.
    pub columns: Vec<CompactedColumn>,
    /// One line per compacted column: `Heading: 1 Value · 2 Value`.
    pub legend: Vec<String>,
}
impl ValuePlan {
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }
    pub fn compacted(&self, column: usize) -> Option<&CompactedColumn> {
        self.columns.iter().find(|c| c.column == column)
    }
    /// What the cell prints: its index when the column compacts, else the literal.
    pub fn cell(&self, column: usize, literal: &str) -> String {
        match self.compacted(column) {
            Some(compacted) => compacted.index(literal),
            None => literal.to_owned(),
        }
    }
}

/// Distinct non-empty values of one column in legend order, and how many
/// non-empty cells they came from.
fn categories(rows: &[Vec<String>], column: usize) -> (Vec<String>, usize) {
    let mut seen: BTreeMap<(String, String), ()> = BTreeMap::new();
    let mut filled = 0;
    for row in rows {
        let literal = row[column].trim();
        if !literal.is_empty() {
            filled += 1;
            seen.entry((literal.to_lowercase(), literal.to_owned()))
                .or_default();
        }
    }
    (
        seen.into_keys().map(|(_, literal)| literal).collect(),
        filled,
    )
}

/// Decide which columns compact and what their legend says. `rows` are the
/// cells as they will print, of the rows that will print; `measure` is the
/// renderer's width of a cell text at the table's type size.
pub fn plan(
    key: &ValueKey,
    columns: &[String],
    headings: &[String],
    rows: &[Vec<String>],
    measure: impl Fn(&str) -> f64,
) -> Result<ValuePlan, String> {
    key.validate(Some(columns))?;
    let count = columns.len();
    if count == 0
        || count > super::table_headings::MAX_COLUMNS
        || headings.len() != count
        || rows.iter().any(|row| row.len() != count)
    {
        return Err("Invalid table value key cells.".into());
    }
    let mut result = ValuePlan::default();
    if key.mode == ValueKeyMode::Off {
        return Ok(result);
    }
    for (column, (column_key, heading)) in columns.iter().zip(headings).enumerate() {
        let chosen = match key.mode {
            ValueKeyMode::Off => false,
            ValueKeyMode::Auto => true,
            ValueKeyMode::Columns => key.columns.contains(column_key),
        };
        if !chosen {
            continue;
        }
        let (values, filled) = categories(rows, column);
        if values.is_empty() {
            continue;
        }
        if values.len() > key.max_categories {
            if key.mode == ValueKeyMode::Columns {
                return Err(format!(
                    "value_key column `{column_key}` has {} distinct values, more than the {} allowed; raise max_categories or drop the column",
                    values.len(),
                    key.max_categories
                ));
            }
            continue;
        }
        if key.mode == ValueKeyMode::Auto {
            // A category is a value that recurs, and a key has at least two
            // entries. Values that each appear once are not categories:
            // indexing them would only move every value from its cell into
            // the legend; one value everywhere is a caption, not a key.
            if values.len() < MIN_MAX_CATEGORIES || values.len() >= filled {
                continue;
            }
            let index = measure(&values.len().to_string());
            let widest = values.iter().map(|v| measure(v)).fold(0_f64, f64::max);
            if !index.is_finite() || !widest.is_finite() || index < 0. || widest < 0. {
                return Err("Invalid table value measurements.".into());
            }
            if widest <= index + 1e-7 {
                continue;
            }
        }
        let heading = if heading.trim().is_empty() {
            column_key.clone()
        } else {
            heading.trim().to_owned()
        };
        let compacted = CompactedColumn {
            column,
            heading,
            values,
        };
        result.legend.push(compacted.legend_line());
        result.columns.push(compacted);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn width(text: &str) -> f64 {
        text.chars().count() as f64 * 1.8
    }
    fn house_connections() -> (Vec<String>, Vec<String>, Vec<Vec<String>>) {
        let columns = ["house_number", "category", "meter_type", "service_length"]
            .map(str::to_owned)
            .to_vec();
        let headings = ["House", "Category", "Meter type", "Svc m"]
            .map(str::to_owned)
            .to_vec();
        let rows = [
            ["H1", "Residential", "Single Phase", "12"],
            ["H2", "Residential", "Single Phase", "25"],
            ["H3", "Commercial", "Three Phase", "8"],
            ["H4", "residential", "", "30"],
            ["H5", "Residential", "Single Phase", ""],
        ]
        .iter()
        .map(|row| row.map(str::to_owned).to_vec())
        .collect();
        (columns, headings, rows)
    }
    #[test]
    fn auto_compacts_few_categories_with_ordered_indexes_and_full_headings() {
        let (columns, headings, rows) = house_connections();
        let plan = plan(&ValueKey::auto(), &columns, &headings, &rows, width).unwrap();
        assert_eq!(
            plan.columns.iter().map(|c| c.column).collect::<Vec<_>>(),
            [1, 2]
        );
        // Case-insensitive order, exact spellings kept, indexes one-based.
        assert_eq!(
            plan.columns[0].values,
            ["Commercial", "Residential", "residential"]
        );
        assert_eq!(
            plan.legend,
            [
                "Category: 1 Commercial · 2 Residential · 3 residential",
                "Meter type: 1 Single Phase · 2 Three Phase",
            ]
        );
        assert_eq!(plan.cell(1, "Residential"), "2");
        assert_eq!(plan.cell(2, "Three Phase"), "2");
        // Empty values print empty and take no index; literal columns pass through.
        assert_eq!(plan.cell(2, ""), "");
        assert_eq!(plan.cell(3, "12"), "12");
        assert_eq!(plan.cell(0, "H1"), "H1");
    }
    #[test]
    fn auto_never_compacts_values_no_wider_than_their_index_or_too_many_categories() {
        let (columns, headings, rows) = house_connections();
        // Five distinct houses exceed three categories; the digit-wide
        // service lengths are not wider than an index.
        let mut narrow = rows.clone();
        for row in &mut narrow {
            row[3] = "1".into();
        }
        let plan = plan(&ValueKey::auto(), &columns, &headings, &narrow, width).unwrap();
        assert!(plan.compacted(0).is_none() && plan.compacted(3).is_none());
        assert_eq!(plan.columns.len(), 2);
        // Raising the ceiling does not index five houses that each appear
        // once: nothing recurs, so nothing is a category. Once one recurs,
        // the column compacts under the raised ceiling.
        let mut wide = ValueKey::auto();
        wide.max_categories = 5;
        let plan = super::plan(&wide, &columns, &headings, &rows, width).unwrap();
        assert!(plan.compacted(0).is_none());
        let mut recurring = rows.clone();
        recurring[4][0] = "H1".into();
        let plan = super::plan(&wide, &columns, &headings, &recurring, width).unwrap();
        assert_eq!(plan.legend[0], "House: 1 H1 · 2 H2 · 3 H3 · 4 H4");
        // One value everywhere is a caption, not a key: auto leaves it
        // literal; the explicit columns mode still compacts it.
        let mut uniform = rows.clone();
        for row in &mut uniform {
            row[1] = "Residential".into();
        }
        let plan = super::plan(&ValueKey::auto(), &columns, &headings, &uniform, width).unwrap();
        assert!(plan.compacted(1).is_none());
        let only = ValueKey {
            mode: ValueKeyMode::Columns,
            max_categories: 3,
            columns: vec!["category".into()],
        };
        assert_eq!(
            super::plan(&only, &columns, &headings, &uniform, width)
                .unwrap()
                .legend,
            ["Category: 1 Residential"]
        );
        // Two rows of two different values are two singletons, not two
        // categories; the explicit columns mode still compacts them.
        let pair = vec![rows[0].clone(), rows[2].clone()];
        assert!(
            super::plan(&ValueKey::auto(), &columns, &headings, &pair, width)
                .unwrap()
                .is_empty()
        );
        let explicit = ValueKey {
            mode: ValueKeyMode::Columns,
            max_categories: 3,
            columns: vec!["category".into()],
        };
        assert_eq!(
            super::plan(&explicit, &columns, &headings, &pair, width)
                .unwrap()
                .legend,
            ["Category: 1 Commercial · 2 Residential"]
        );
    }
    #[test]
    fn columns_mode_compacts_exactly_the_listed_columns_or_refuses_naming_the_count() {
        let (columns, headings, rows) = house_connections();
        let key = ValueKey {
            mode: ValueKeyMode::Columns,
            max_categories: 3,
            columns: vec!["meter_type".into()],
        };
        let plan = plan(&key, &columns, &headings, &rows, width).unwrap();
        assert_eq!(plan.legend, ["Meter type: 1 Single Phase · 2 Three Phase"]);
        assert!(plan.compacted(1).is_none());
        let key = ValueKey {
            columns: vec!["house_number".into()],
            ..key
        };
        let refused = super::plan(&key, &columns, &headings, &rows, width).unwrap_err();
        assert!(
            refused.contains("`house_number`") && refused.contains("5 distinct"),
            "{refused}"
        );
        assert!(refused.contains("raise max_categories"));
        let unbound = ValueKey {
            mode: ValueKeyMode::Columns,
            max_categories: 3,
            columns: vec!["village".into()],
        };
        assert!(super::plan(&unbound, &columns, &headings, &rows, width)
            .unwrap_err()
            .contains("`village`"));
    }
    #[test]
    fn off_and_empty_columns_print_literally() {
        let (columns, headings, rows) = house_connections();
        assert!(plan(&ValueKey::off(), &columns, &headings, &rows, width)
            .unwrap()
            .is_empty());
        let blank: Vec<Vec<String>> = vec![vec![String::new(); 4]; 3];
        assert!(plan(&ValueKey::auto(), &columns, &headings, &blank, width)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn setup_bounds_are_refused_and_bad_cells_are_refused() {
        let mut key = ValueKey::auto();
        key.max_categories = 1;
        assert!(key.validate(None).is_err());
        key.max_categories = 27;
        assert!(key.validate(None).is_err());
        key.max_categories = 26;
        assert!(key.validate(None).is_ok());
        let key = ValueKey {
            mode: ValueKeyMode::Columns,
            max_categories: 3,
            columns: vec![],
        };
        assert!(key.validate(None).is_err());
        let key = ValueKey {
            columns: vec!["a".into(), "a".into()],
            ..key
        };
        assert!(key.validate(None).unwrap_err().contains("repeated"));
        let (columns, headings, _) = house_connections();
        assert!(plan(&ValueKey::auto(), &columns, &headings, &[vec![]], width).is_err());
        assert!(plan(&ValueKey::auto(), &[], &[], &[], width).is_err());
        let (_, _, rows) = house_connections();
        assert!(plan(&ValueKey::auto(), &columns, &headings, &rows, |_| f64::NAN).is_err());
    }
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
