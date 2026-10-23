//! Phase C bootstrap: a Polars-backed table view for one design layer.
//!
//! This module is feature-gated behind `enable_dataframes` while we prove
//! wasm32 build viability and bundle size. The first contract is deliberately
//! small: preserve feature id + geometry + properties round-trips in a real
//! Polars `DataFrame`, then expose row windows to ds-web through the session
//! handle.

use crate::session::{Feature, Geometry};
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

pub const FEATURE_ID_COLUMN: &str = "feature_id";
pub const GEOMETRY_COLUMN: &str = "geometry";
pub const PROPERTIES_COLUMN: &str = "properties";

#[derive(Clone, Debug)]
pub struct LayerTable {
    layer_name: String,
    df: DataFrame,
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
        let layer_name = layer_name.into();
        let mut ids = Vec::with_capacity(features.len());
        let mut geometries = Vec::with_capacity(features.len());
        let mut properties = Vec::with_capacity(features.len());

        for feature in features {
            ids.push(feature.id.0.clone());
            geometries
                .push(serde_json::to_string(&feature.geometry).map_err(|err| err.to_string())?);
            properties
                .push(serde_json::to_string(&feature.properties).map_err(|err| err.to_string())?);
        }

        let df = DataFrame::new(
            features.len(),
            vec![
                Series::new(FEATURE_ID_COLUMN.into(), ids).into(),
                Series::new(GEOMETRY_COLUMN.into(), geometries).into(),
                Series::new(PROPERTIES_COLUMN.into(), properties).into(),
            ],
        )
        .map_err(|err| err.to_string())?;

        Ok(Self { layer_name, df })
    }

    pub fn layer_name(&self) -> &str {
        &self.layer_name
    }

    pub fn height(&self) -> usize {
        self.df.height()
    }

    pub fn dataframe(&self) -> &DataFrame {
        &self.df
    }

    pub fn feature_ids(&self) -> Result<Vec<String>, String> {
        let ids = self
            .df
            .column(FEATURE_ID_COLUMN)
            .map_err(|err| err.to_string())?
            .str()
            .map_err(|err| err.to_string())?;

        // Polars 0.55 no longer offers `into_iter` on a `&ChunkedArray`.
        Ok(ids
            .iter()
            .map(|id| id.unwrap_or_default().to_string())
            .collect())
    }

    pub fn get_rows(&self, offset: usize, limit: usize) -> Result<RowWindow, String> {
        let total_rows = self.df.height();
        let start = offset.min(total_rows);
        let end = start.saturating_add(limit).min(total_rows);
        let mut rows = Vec::with_capacity(end.saturating_sub(start));

        for row_index in start..end {
            rows.push(self.row_at(row_index)?);
        }

        Ok(RowWindow {
            layer_name: self.layer_name.clone(),
            offset,
            limit,
            total_rows,
            rows,
        })
    }

    pub fn get_row(&self, feature_id: &str) -> Result<Option<LayerRow>, String> {
        let ids = self.feature_id_column()?;
        for row_index in 0..self.df.height() {
            if ids.get(row_index) == Some(feature_id) {
                return Ok(Some(self.row_at(row_index)?));
            }
        }
        Ok(None)
    }

    pub fn insert_feature(&mut self, feature: Feature) -> Result<(), String> {
        if self.get_row(&feature.id.0)?.is_some() {
            return Err(format!(
                "layer `{}` already contains feature `{}`",
                self.layer_name, feature.id.0
            ));
        }

        let mut features = self.to_features()?;
        features.push(feature);
        self.replace_all_features(&features)
    }

    pub fn replace_feature(&mut self, feature: Feature) -> Result<Option<Feature>, String> {
        let mut features = self.to_features()?;
        let old = match features.iter().position(|row| row.id == feature.id) {
            Some(pos) => Some(std::mem::replace(&mut features[pos], feature)),
            None => {
                features.push(feature);
                None
            }
        };
        self.replace_all_features(&features)?;
        Ok(old)
    }

    pub fn remove_feature(&mut self, feature_id: &str) -> Result<Option<Feature>, String> {
        let mut features = self.to_features()?;
        let Some(pos) = features.iter().position(|row| row.id.0 == feature_id) else {
            return Ok(None);
        };
        let removed = features.remove(pos);
        self.replace_all_features(&features)?;
        Ok(Some(removed))
    }

    /// Bulk-remove features by local feature id in ONE table rebuild.
    ///
    /// [`Self::remove_feature`] does a full `to_features` + `from_features`
    /// JSON round-trip per call, so removing a scope's worth of rows one at a
    /// time is O(count * H) — the edit-exit / batch-delete O(N^2) freeze. This
    /// materializes the layer once, partitions out the requested ids, and
    /// rebuilds the DataFrame a single time. Returns the removed features in
    /// table order; ids not present are silently skipped.
    pub fn remove_features(&mut self, feature_ids: &HashSet<&str>) -> Result<Vec<Feature>, String> {
        if feature_ids.is_empty() {
            return Ok(Vec::new());
        }
        let features = self.to_features()?;
        let mut removed = Vec::new();
        let mut retained = Vec::with_capacity(features.len());
        for feature in features {
            if feature_ids.contains(feature.id.0.as_str()) {
                removed.push(feature);
            } else {
                retained.push(feature);
            }
        }
        if !removed.is_empty() {
            self.replace_all_features(&retained)?;
        }
        Ok(removed)
    }

    pub fn edit_cell(
        &mut self,
        feature_id: &str,
        column: &str,
        value: Value,
    ) -> Result<Option<CellEditDiff>, String> {
        let mut features = self.to_features()?;
        let Some(pos) = features.iter().position(|row| row.id.0 == feature_id) else {
            return Ok(None);
        };
        let before = features[pos].properties.get(column).cloned();
        features[pos]
            .properties
            .insert(column.to_string(), value.clone());
        self.replace_all_features(&features)?;
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

        let mut features = self.to_features()?;
        let positions: HashMap<String, usize> = features
            .iter()
            .enumerate()
            .map(|(pos, feature)| (feature.id.0.clone(), pos))
            .collect();
        let mut result = BulkCellEditResult::default();

        for feature_id in feature_ids {
            let Some(pos) = positions.get(feature_id).copied() else {
                result.missing_feature_ids.push(feature_id.clone());
                continue;
            };
            for (column, value) in values {
                let before = features[pos].properties.get(column).cloned();
                if before.as_ref() == Some(value) {
                    continue;
                }
                features[pos]
                    .properties
                    .insert(column.clone(), value.clone());
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

        if !result.changed.is_empty() {
            self.replace_all_features(&features)?;
        }
        Ok(result)
    }

    pub fn delete_column(&mut self, column: &str) -> Result<usize, String> {
        let mut features = self.to_features()?;
        let mut count = 0;
        for feature in &mut features {
            if feature.properties.remove(column).is_some() {
                count += 1;
            }
        }
        if count > 0 {
            self.replace_all_features(&features)?;
        }
        Ok(count)
    }

    pub fn to_features(&self) -> Result<Vec<Feature>, String> {
        let mut features = Vec::with_capacity(self.df.height());
        for row_index in 0..self.df.height() {
            let row = self.row_at(row_index)?;
            features.push(Feature::with_properties(
                row.feature_id,
                row.geometry,
                row.properties,
            ));
        }
        Ok(features)
    }

    fn row_at(&self, row_index: usize) -> Result<LayerRow, String> {
        let ids = self
            .df
            .column(FEATURE_ID_COLUMN)
            .map_err(|err| err.to_string())?
            .str()
            .map_err(|err| err.to_string())?;
        let geometries = self
            .df
            .column(GEOMETRY_COLUMN)
            .map_err(|err| err.to_string())?
            .str()
            .map_err(|err| err.to_string())?;
        let properties = self
            .df
            .column(PROPERTIES_COLUMN)
            .map_err(|err| err.to_string())?
            .str()
            .map_err(|err| err.to_string())?;

        let feature_id = ids
            .get(row_index)
            .ok_or_else(|| format!("missing `{FEATURE_ID_COLUMN}` at row {row_index}"))?;
        let geometry_json = geometries
            .get(row_index)
            .ok_or_else(|| format!("missing `{GEOMETRY_COLUMN}` at row {row_index}"))?;
        let geometry: Geometry =
            serde_json::from_str(geometry_json).map_err(|err| err.to_string())?;
        let properties_json = properties
            .get(row_index)
            .ok_or_else(|| format!("missing `{PROPERTIES_COLUMN}` at row {row_index}"))?;
        let properties: Map<String, Value> =
            serde_json::from_str(properties_json).map_err(|err| err.to_string())?;

        Ok(LayerRow {
            row_index,
            feature_id: feature_id.to_string(),
            geometry,
            properties,
        })
    }

    fn feature_id_column(&self) -> Result<&StringChunked, String> {
        self.df
            .column(FEATURE_ID_COLUMN)
            .map_err(|err| err.to_string())?
            .str()
            .map_err(|err| err.to_string())
    }

    fn replace_all_features(&mut self, features: &[Feature]) -> Result<(), String> {
        let next = LayerTable::from_features(self.layer_name.clone(), features)?;
        self.df = next.df;
        Ok(())
    }
}

#[cfg(test)]
#[path = "dataframe_tests.rs"]
mod tests;
