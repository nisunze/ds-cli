//! The dead-letter summary: how many survey payloads ds-brain rejected for a
//! project, and the last one's facts, for the survey dashboard badge.
//! Rejections are recorded by Brain when it refuses a payload; this read
//! only reports them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/deadletter";
/// Aggregating a large project's trail can take minutes.
const TIMEOUT_MS: u64 = 300_000;
pub const INVALID: &str = "deadletter_answer_invalid";

/// One project's dead-letter summary as the survey dashboard shows it.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[cfg_attr(
    all(feature = "wasm", target_arch = "wasm32"),
    tsify(hashmap_as_object)
)]
pub struct DeadLetterSummary {
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_doc_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_form_slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_gcs_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_email: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub by_reason: BTreeMap<String, u64>,
}

/// What Brain records: every member it omits takes its default, so nothing
/// recorded reads as the zero summary.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Recorded {
    count: u64,
    last_at: Option<String>,
    last_doc_id: Option<String>,
    last_form_slug: Option<String>,
    last_reason: Option<String>,
    last_stage: Option<String>,
    last_message: Option<String>,
    last_gcs_uri: Option<String>,
    last_user_email: Option<String>,
    by_reason: BTreeMap<String, u64>,
}

impl From<Recorded> for DeadLetterSummary {
    fn from(recorded: Recorded) -> Self {
        Self {
            count: recorded.count,
            last_at: recorded.last_at,
            last_doc_id: recorded.last_doc_id,
            last_form_slug: recorded.last_form_slug,
            last_reason: recorded.last_reason,
            last_stage: recorded.last_stage,
            last_message: recorded.last_message,
            last_gcs_uri: recorded.last_gcs_uri,
            last_user_email: recorded.last_user_email,
            by_reason: recorded.by_reason,
        }
    }
}

/// One project's dead-letter summary; nothing recorded reads as a zero count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub project: String,
}

impl Exchange for Summary {
    type Output = DeadLetterSummary;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body =
            json!({"action": "get_summary", "project_id": project_reference(&self.project)?});
        Ok(Request::post(PATH, body).timeout_ms(TIMEOUT_MS).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Recorded::deserialize(data(&body))
            .map(|recorded| Admitted::Done(recorded.into()))
            .map_err(|_| {
                Refusal::new(
                    INVALID,
                    "The dead-letter service answered an invalid summary.",
                )
            })
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// The typed browser export Svelte calls; it runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::Summary;
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// One project's dead-letter summary.
    #[wasm_bindgen(js_name = deadLetterSummary, unchecked_return_type = "DeadLetterSummary")]
    pub async fn dead_letter_summary(
        host: GatewayHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, Summary { project }).await?)
    }
}

#[cfg(test)]
#[path = "deadletter_tests.rs"]
mod tests;
