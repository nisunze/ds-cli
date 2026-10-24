//! The attribute-table view of one design layer.
//!
//! `SessionHandle` keeps one `LayerTable` per mounted layer, in source row
//! order, and serves row windows, single rows and cell edits from it. The
//! table is design working state, not compute: filter, sort and value
//! profiles run in the Network page unit over Arrow IPC (`ds_geo::table`).
//! Rows are the session's own `Feature` values, so no dataframe engine is
//! linked into the Kernel. (Until 24 Oct 2026 the rows were held as JSON
//! text columns of a Polars `DataFrame`; every read and edit decoded them
//! back to these same values, and the Kernel build's exact-float,
//! order-preserving JSON made that round trip the identity.)

use crate::session::{Feature, Geometry};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct LayerTable {
    layer_name: String,
    rows: Vec<Feature>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(missing_as_null)
)]
pub struct LayerRow {
    pub row_index: usize,
    pub feature_id: String,
    pub geometry: Geometry,
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub properties: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
pub struct RowWindow {
    pub layer_name: String,
    pub offset: usize,
    pub limit: usize,
    pub total_rows: usize,
    pub rows: Vec<LayerRow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(missing_as_null)
)]
pub struct CellEditDiff {
    pub layer_name: String,
    pub row_index: usize,
    pub feature_id: String,
    pub column: String,
    /// The cell value; `null` when the cell was empty.
    #[serde(deserialize_with = "crate::reply::written_nullable")]
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "unknown")
    )]
    pub before: Option<Value>,
    #[serde(deserialize_with = "crate::reply::written_nullable")]
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "unknown")
    )]
    pub after: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(missing_as_null)
)]
pub struct BulkCellEditResult {
    pub changed: Vec<CellEditDiff>,
    pub missing_feature_ids: Vec<String>,
}

impl LayerTable {
    pub fn from_features(
        layer_name: impl Into<String>,
        features: &[Feature],
    ) -> Result<Self, String> {
        Ok(Self {
            layer_name: layer_name.into(),
            rows: features.to_vec(),
        })
    }

    pub fn layer_name(&self) -> &str {
        &self.layer_name
    }

    pub fn height(&self) -> usize {
        self.rows.len()
    }

    pub fn feature_ids(&self) -> Result<Vec<String>, String> {
        Ok(self.rows.iter().map(|row| row.id.0.clone()).collect())
    }

    pub fn get_rows(&self, offset: usize, limit: usize) -> Result<RowWindow, String> {
        let total_rows = self.rows.len();
        let start = offset.min(total_rows);
        let end = start.saturating_add(limit).min(total_rows);
        let rows = (start..end)
            .map(|row_index| self.row_at(row_index))
            .collect();

        Ok(RowWindow {
            layer_name: self.layer_name.clone(),
            offset,
            limit,
            total_rows,
            rows,
        })
    }

    pub fn get_row(&self, feature_id: &str) -> Result<Option<LayerRow>, String> {
        Ok(self
            .rows
            .iter()
            .position(|row| row.id.0 == feature_id)
            .map(|row_index| self.row_at(row_index)))
    }

    pub fn insert_feature(&mut self, feature: Feature) -> Result<(), String> {
        if self.rows.iter().any(|row| row.id.0 == feature.id.0) {
            return Err(format!(
                "layer `{}` already contains feature `{}`",
                self.layer_name, feature.id.0
            ));
        }
        self.rows.push(feature);
        Ok(())
    }

    pub fn replace_feature(&mut self, feature: Feature) -> Result<Option<Feature>, String> {
        Ok(
            match self.rows.iter().position(|row| row.id == feature.id) {
                Some(pos) => Some(std::mem::replace(&mut self.rows[pos], feature)),
                None => {
                    self.rows.push(feature);
                    None
                }
            },
        )
    }

    pub fn remove_feature(&mut self, feature_id: &str) -> Result<Option<Feature>, String> {
        Ok(self
            .rows
            .iter()
            .position(|row| row.id.0 == feature_id)
            .map(|pos| self.rows.remove(pos)))
    }

    /// Bulk-remove features by local feature id in one pass over the rows,
    /// so removing a scope's worth of rows stays O(H) rather than O(count * H)
    /// (the edit-exit / batch-delete freeze). Returns the removed features in
    /// table order; ids not present are silently skipped.
    pub fn remove_features(&mut self, feature_ids: &HashSet<&str>) -> Result<Vec<Feature>, String> {
        if feature_ids.is_empty() {
            return Ok(Vec::new());
        }
        let (removed, retained): (Vec<Feature>, Vec<Feature>) = std::mem::take(&mut self.rows)
            .into_iter()
            .partition(|feature| feature_ids.contains(feature.id.0.as_str()));
        self.rows = retained;
        Ok(removed)
    }

    pub fn edit_cell(
        &mut self,
        feature_id: &str,
        column: &str,
        value: Value,
    ) -> Result<Option<CellEditDiff>, String> {
        let Some(pos) = self.rows.iter().position(|row| row.id.0 == feature_id) else {
            return Ok(None);
        };
        let properties = &mut self.rows[pos].properties;
        let before = properties.get(column).cloned();
        properties.insert(column.to_string(), value.clone());
        Ok(Some(CellEditDiff {
            layer_name: self.layer_name.clone(),
            row_index: pos,
            feature_id: feature_id.to_string(),
            column: column.to_string(),
            before,
            after: Some(value),
        }))
    }

    pub fn edit_cells(
        &mut self,
        feature_ids: &[String],
        values: &Map<String, Value>,
    ) -> Result<BulkCellEditResult, String> {
        for column in values.keys() {
            if column.trim().is_empty() {
                return Err("column name is required".to_string());
            }
        }
        if feature_ids.is_empty() || values.is_empty() {
            return Ok(BulkCellEditResult::default());
        }

        // A repeated table id resolves to its last row, as it always has.
        let positions: HashMap<&str, usize> = self
            .rows
            .iter()
            .enumerate()
            .map(|(pos, feature)| (feature.id.0.as_str(), pos))
            .collect();
        let mut targets = Vec::with_capacity(feature_ids.len());
        let mut result = BulkCellEditResult::default();
        for feature_id in feature_ids {
            match positions.get(feature_id.as_str()) {
                Some(pos) => targets.push((feature_id, *pos)),
                None => result.missing_feature_ids.push(feature_id.clone()),
            }
        }

        for (feature_id, pos) in targets {
            for (column, value) in values {
                let properties = &mut self.rows[pos].properties;
                let before = properties.get(column).cloned();
                if before.as_ref() == Some(value) {
                    continue;
                }
                properties.insert(column.clone(), value.clone());
                result.changed.push(CellEditDiff {
                    layer_name: self.layer_name.clone(),
                    row_index: pos,
                    feature_id: feature_id.clone(),
                    column: column.clone(),
                    before,
                    after: Some(value.clone()),
                });
            }
        }
        Ok(result)
    }

    pub fn delete_column(&mut self, column: &str) -> Result<usize, String> {
        let mut count = 0;
        for feature in &mut self.rows {
            if feature.properties.remove(column).is_some() {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn to_features(&self) -> Result<Vec<Feature>, String> {
        Ok(self.rows.clone())
    }

    fn row_at(&self, row_index: usize) -> LayerRow {
        let feature = &self.rows[row_index];
        LayerRow {
            row_index,
            feature_id: feature.id.0.clone(),
            geometry: feature.geometry.clone(),
            properties: feature.properties.clone(),
        }
    }
}

#[cfg(test)]
#[path = "dataframe_tests.rs"]
mod tests;
