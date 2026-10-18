//! Mechanical SQLite effects shared by native and browser adapters.
//! Domain owners provide SQL and continuation decisions; hosts only execute it.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Read,
    Write,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Statement {
    pub sql: String,
    pub bind: Vec<Value>,
    pub max_rows: u32,
    pub max_bytes: u32,
}
impl Statement {
    pub fn new(sql: &str, bind: Vec<Value>, max_rows: u32, max_bytes: u32) -> Self {
        Self {
            sql: sql.into(),
            bind,
            max_rows,
            max_bytes,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.sql.is_empty()
            || self.sql.len() > 64 * 1024
            || self.bind.len() > 128
            || serde_json::to_vec(&self.bind)
                .map_err(|e| e.to_string())?
                .len()
                > 64 * 1024 * 1024
            || self.max_rows > 8192
            || self.max_bytes > 64 * 1024 * 1024
        {
            return Err("sqlite_effect_invalid: statement exceeds its bounds".into());
        }
        for value in &self.bind {
            scalar(value)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultRows {
    pub rows: Vec<Vec<Value>>,
    pub changes: u64,
}
impl ResultRows {
    pub fn validate(&self, statement: &Statement) -> Result<(), String> {
        if self.rows.len() > statement.max_rows as usize
            || serde_json::to_vec(&self.rows)
                .map_err(|e| e.to_string())?
                .len()
                > statement.max_bytes as usize
        {
            return Err("sqlite_effect_invalid: result exceeds its bounds".into());
        }
        for row in &self.rows {
            for value in row {
                scalar(value)?;
            }
        }
        Ok(())
    }
}
fn scalar(value: &Value) -> Result<(), String> {
    if value.is_null() || value.is_string() {
        return Ok(());
    }
    if let Some(number) = value.as_f64() {
        if number.is_finite() && number.abs() <= 9_007_199_254_740_991.0 {
            return Ok(());
        }
    }
    Err("sqlite_effect_invalid: expected bounded SQLite scalar".into())
}
