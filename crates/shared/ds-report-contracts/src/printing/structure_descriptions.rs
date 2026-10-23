//! Exact print-only binding to a staking workbook. This never edits a model.
//! The binding is the contract; the Reporter reads the workbook and binds it
//! (`ds-network-reporter` `structure_descriptions`).
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "ds.mv-structure-descriptions/v1";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Column {
    /// Exact spreadsheet column letter, within A..AN.
    pub column: String,
    /// Exact source header, not a guessed alias.
    pub header: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Columns {
    pub engineering_number: Column,
    pub engineering_name: Column,
    pub x_m: Column,
    pub y_m: Column,
    pub description: Column,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub schema: String,
    pub project_id: String,
    pub model_revision: String,
    /// Held source path; the native host reads its bytes once.
    pub workbook_path: String,
    pub workbook_sha256: String,
    pub sheet: String,
    /// One-based source header row.
    pub header_row: usize,
    /// Declared coordinate-rounding tolerance, bounded to 0..0.05 metres.
    pub xy_tolerance_m: f64,
    pub columns: Columns,
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub entity_id: String,
    pub engineering_number: String,
    pub engineering_name: String,
    pub x_m: f64,
    pub y_m: f64,
}

pub fn schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Binding)).expect("description binding schema")
}
