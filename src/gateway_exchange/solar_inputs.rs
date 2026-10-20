//! The last Solar input edits a machine queued before Solar authoring moved
//! to `ds solar`: each queued row is sent once to ds-brain (`POST
//! /api/v1/solar`), as `save_input`, or as `project_commit` for a city that was
//! created on this machine, and admitted by the receipt that names it. Nothing
//! new is ever queued.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/solar";
/// A city's committed snapshot can be large; its receipt is small.
const TIMEOUT_MS: u64 = 120_000;
pub const INPUT_INVALID: &str = "solar_input_queue_invalid";
pub const RECEIPT_INVALID: &str = "solar_input_receipt_invalid";

/// One row of the machine's Solar input queue, as the queue stored it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueuedInput {
    #[serde(default)]
    pub operation: Option<String>,
    pub root: String,
    pub template_id: String,
    pub subcollection: String,
    pub doc_id: String,
    pub payload: Map<String, Value>,
}

fn invalid(what: &str) -> Refusal {
    Refusal::new(INPUT_INVALID, format!("A queued Solar input names one valid {what}."))
}

impl QueuedInput {
    /// The project a Solar root `eds_project/{project}/eds_solar` belongs to.
    fn project_of(root: &str) -> Option<&str> {
        let mut parts = root.split('/');
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some("eds_project"), Some(project), Some(_), None)
                if crate::execution_context::valid_project(project) =>
            {
                Some(project)
            }
            _ => None,
        }
    }

    fn committing(&self) -> Result<bool, Refusal> {
        match self.operation.as_deref() {
            None => Ok(false),
            Some("project_commit") => Ok(true),
            Some(_) => Err(invalid("operation")),
        }
    }
}

/// Send one queued row once.
#[derive(Clone, Debug, PartialEq)]
pub struct SendQueued(pub QueuedInput);

impl Exchange for SendQueued {
    type Output = ();

    fn plan(&self) -> Result<Plan, Refusal> {
        let row = &self.0;
        QueuedInput::project_of(&row.root).ok_or_else(|| invalid("Solar root"))?;
        for (what, value) in [("city", &row.template_id), ("document", &row.doc_id), ("subcollection", &row.subcollection)] {
            if value.trim().is_empty() || value.len() > 256 {
                return Err(invalid(what));
            }
        }
        let body = if row.committing()? {
            let snapshot = row
                .payload
                .get("snapshot_json")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("city snapshot"))?;
            json!({
                "action": "project_commit", "root": row.root, "template_id": row.template_id,
                "project_commit": {"expected_base": "", "snapshot_json": snapshot},
            })
        } else {
            if row.payload.is_empty() {
                return Err(invalid("document body"));
            }
            json!({
                "action": "save_input", "template_id": row.template_id,
                "subcollection": row.subcollection, "doc_id": row.doc_id,
                "data": row.payload, "root": row.root,
            })
        };
        Ok(Request::post(PATH, body).timeout_ms(TIMEOUT_MS).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let row = self.0;
        let answer = data(&body);
        let text = |key: &str| answer.get(key).and_then(Value::as_str);
        let admitted = if row.committing()? {
            let expected_sha = row.payload.get("snapshot_sha256").and_then(Value::as_str);
            text("root") == Some(row.root.as_str())
                && text("city_id") == Some(row.template_id.as_str())
                && expected_sha.is_some()
                && text("client_snapshot_sha256") == expected_sha
                && text("input_base_fingerprint")
                    .is_some_and(crate::report_export::exact_lower_sha256)
        } else {
            text("template_id") == Some(row.template_id.as_str())
                && text("subcollection") == Some(row.subcollection.as_str())
                && text("doc_id") == Some(row.doc_id.as_str())
        };
        if !admitted {
            return Err(Refusal::new(
                RECEIPT_INVALID,
                "ds-brain answered a receipt for another Solar input.",
            ));
        }
        Ok(Admitted::Done(()))
    }

    fn project(&self) -> Option<&str> {
        QueuedInput::project_of(&self.0.root)
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{QueuedInput, SendQueued};
    use crate::gateway_exchange::browser::{run, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// Send one row of the machine's Solar input queue once; resolves when
    /// ds-brain's receipt names it.
    #[wasm_bindgen(js_name = solarQueuedInputSend)]
    pub async fn solar_queued_input_send(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] row: JsValue,
    ) -> Result<(), JsValue> {
        let row: QueuedInput = serde_wasm_bindgen_row(row)?;
        run(&host, SendQueued(row)).await
    }

    fn serde_wasm_bindgen_row(row: JsValue) -> Result<QueuedInput, JsValue> {
        tsify::serde_wasm_bindgen::from_value(row)
            .map_err(|error| js_sys::Error::new(&format!("The queued Solar input is unreadable: {error}")).into())
    }
}

#[cfg(test)]
#[path = "solar_inputs_tests.rs"]
mod tests;
