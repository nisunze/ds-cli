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

/// Execute one bounded statement on a rusqlite connection. This is the one
/// executor every host shares: Core's protected Store, the native layer
/// stores and the browser storage worker (rusqlite over sqlite-wasm-rs).
#[cfg(feature = "durable-store")]
pub fn execute(
    connection: &rusqlite::Connection,
    effect: &Statement,
) -> Result<ResultRows, String> {
    use rusqlite::types::{Value as SqlValue, ValueRef};
    effect.validate()?;
    let values = effect
        .bind
        .iter()
        .map(|value| match value {
            Value::Null => Ok(SqlValue::Null),
            Value::String(text) => Ok(SqlValue::Text(text.clone())),
            Value::Number(number) => number
                .as_i64()
                .map(SqlValue::Integer)
                .or_else(|| number.as_f64().map(SqlValue::Real))
                .ok_or_else(|| "invalid SQL number".to_string()),
            _ => Err("invalid SQL scalar".to_string()),
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut statement = connection.prepare(&effect.sql).map_err(|e| e.to_string())?;
    let columns = statement.column_count();
    let mut cursor = statement
        .query(rusqlite::params_from_iter(values))
        .map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut bytes = 2usize;
    while let Some(row) = cursor.next().map_err(|e| e.to_string())? {
        if rows.len() >= effect.max_rows as usize {
            return Err("SQLite row bound exceeded".into());
        }
        let mut values = Vec::with_capacity(columns);
        for column in 0..columns {
            let value = match row.get_ref(column).map_err(|e| e.to_string())? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(n) => Value::from(n),
                ValueRef::Real(n) => Value::from(n),
                ValueRef::Text(text) => {
                    if text.len() > effect.max_bytes as usize {
                        return Err("SQLite byte bound exceeded".into());
                    }
                    Value::String(std::str::from_utf8(text).map_err(|e| e.to_string())?.into())
                }
                ValueRef::Blob(_) => return Err("SQLite blob needs a byte-handle effect".into()),
            };
            bytes += serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() + 1;
            if bytes > effect.max_bytes as usize {
                return Err("SQLite byte bound exceeded".into());
            }
            values.push(value);
        }
        bytes += 2;
        rows.push(values);
    }
    let result = ResultRows {
        rows,
        changes: connection.changes(),
    };
    result.validate(effect)?;
    Ok(result)
}

#[cfg(all(test, feature = "durable-store"))]
#[path = "sql_effect_tests.rs"]
mod tests;
