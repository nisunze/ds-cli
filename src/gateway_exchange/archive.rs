//! The platform's transformer delete ledger, newest deletion first (`POST
//! /api/v1/data` `query_table` `deleted_transformers`, NDJSON). Its trailing
//! `{__type:"summary"}` line carries the count, or the failure that cut the
//! stream; rows travel as served. Platform administrators only.

use serde::Serialize;
use serde_json::{json, Value};

use super::{Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/data";
const TIMEOUT_MS: u64 = 30_000;
/// The window ds-brain bounds server-side, plus its summary line.
pub const ROWS_MAX: usize = 20_000;
const CAP: u64 = 64 * 1024 * 1024;
pub const FAILED: &str = "backup_ledger_stream_failed";

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Ledger {
    pub rows: Vec<Value>,
    pub count: u64,
}

/// The ledger of the last `days` (ds-brain's default 30, at most 365).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeletedTransformers {
    pub days: Option<u32>,
    pub limit: Option<u32>,
}

impl Exchange for DeletedTransformers {
    type Output = Ledger;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = json!({"action": "query_table", "table": "deleted_transformers"});
        if let Some(days) = self.days {
            body["days"] = days.into();
        }
        if let Some(limit) = self.limit {
            body["limit"] = limit.into();
        }
        Ok(Request::post(PATH, body)
            .timeout_ms(TIMEOUT_MS)
            .response_cap(CAP)
            .ndjson(ROWS_MAX + 1)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let mut rows = Vec::new();
        let mut summary = None;
        for line in body.as_array().into_iter().flatten() {
            if line.get("__type").and_then(Value::as_str) == Some("summary") {
                summary = Some(line);
            } else if line.is_object() {
                rows.push(line.clone());
            }
        }
        if summary.and_then(|line| line.get("status")).and_then(Value::as_str) == Some("error") {
            return Err(Refusal::new(FAILED, "The backup ledger stream failed."));
        }
        let count = summary
            .and_then(|line| line.get("count"))
            .and_then(Value::as_u64)
            .filter(|count| *count > 0)
            .unwrap_or(rows.len() as u64);
        Ok(Admitted::Done(Ledger { rows, count }))
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::DeletedTransformers;
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// The platform's transformer delete ledger, newest deletion first.
    #[wasm_bindgen(js_name = deletedTransformers, unchecked_return_type = "{ rows: Record<string, unknown>[]; count: number }")]
    pub async fn deleted_transformers(
        host: GatewayHost,
        days: Option<u32>,
        limit: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, DeletedTransformers { days, limit }).await?)
    }
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
