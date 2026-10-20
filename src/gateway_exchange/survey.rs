//! Survey entries over the gateway: one form's entries as Brain streams them
//! (`query_entries`, NDJSON ending in a summary line), one entry correction or
//! deletion from the capture outbox, and the audit of entries a person chose
//! to discard. Brain authorizes each project and form; the survey read policy
//! decides what the streamed rows mean.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, project_reference, Admitted, Exchange, Method, Plan, Refusal, Request};

const DATA_PATH: &str = "/api/v1/data";
const MUTATE_PATH: &str = "/api/v1/entries/mutate";
const DEADLETTER_PATH: &str = "/api/v1/deadletter";
/// A whole form of a large project, with every feature's geometry.
const ENTRIES_CAP: u64 = 256 * 1024 * 1024;
const ENTRIES_MAX_LINES: usize = 1_000_000;
/// A large form streams for minutes after its freshness sync.
const ENTRIES_TIMEOUT_MS: u64 = 900_000;
/// Brain's own mutation deadline and receipt bound.
const MUTATE_TIMEOUT_MS: u64 = 120_000;
const MUTATE_CAP: u64 = 64 * 1024;
const DISCARD_MAX_ENTRIES: usize = 5_000;
const SLUG_MAX_CHARS: usize = 128;
pub const INPUT_INVALID: &str = "survey_input_invalid";
pub const INVALID: &str = "survey_answer_invalid";

fn input_invalid(message: &str) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

/// Which entries of a form a read asks for; every member is optional and an
/// empty filter reads the whole form.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct SurveyFilter {
    /// `[west, south, east, north]` in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub bbox: Option<Vec<f64>>,
    /// One administrative unit, as Brain resolves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(optional, type = "Record<string, string>")
    )]
    pub admin_boundary: Option<Map<String, Value>>,
    /// A GeoJSON boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(optional, type = "Record<string, unknown>")
    )]
    pub boundary: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub date_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub date_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub surveyors: Option<Vec<String>>,
}

/// One form's entries, filtered, optionally only those changed since a
/// cutoff and with their tombstones.
#[derive(Clone, Debug, PartialEq)]
pub struct Entries {
    pub project: String,
    pub form: String,
    pub filter: SurveyFilter,
    pub updated_after: Option<String>,
    pub include_tombstones: bool,
}

fn slug(text: &str) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= SLUG_MAX_CHARS
        && !text.chars().any(char::is_control)
}

impl Exchange for Entries {
    /// Every streamed line in order: the entries, then Brain's summary.
    type Output = Vec<Value>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let project = project_reference(&self.project)?;
        if !slug(&self.form) {
            return Err(input_invalid("A survey read names one form."));
        }
        if self
            .filter
            .bbox
            .as_ref()
            .is_some_and(|bbox| bbox.len() != 4)
        {
            return Err(input_invalid(
                "A survey bbox is west, south, east and north.",
            ));
        }
        let mut body = json!({"action": "query_entries", "eds_project_id": project,
            "form_slugs": [self.form]});
        let filter = &self.filter;
        if let Some(bbox) = &filter.bbox {
            body["bbox"] = json!(bbox);
        }
        if let Some(admin) = &filter.admin_boundary {
            body["filter_admin_boundary"] = Value::Object(admin.clone());
        }
        if let Some(boundary) = &filter.boundary {
            body["filter_boundary"] = Value::Object(boundary.clone());
        }
        if let Some(from) = filter.date_from.as_deref().filter(|date| !date.is_empty()) {
            body["date_from"] = json!(from);
        }
        if let Some(to) = filter.date_to.as_deref().filter(|date| !date.is_empty()) {
            body["date_to"] = json!(to);
        }
        if let Some(surveyors) = filter.surveyors.as_ref().filter(|list| !list.is_empty()) {
            body["surveyors"] = json!(surveyors);
        }
        if let Some(after) = self
            .updated_after
            .as_deref()
            .filter(|after| !after.is_empty())
        {
            body["updated_after"] = json!(after);
        }
        if self.include_tombstones {
            body["exclude_deleted"] = json!(false);
        }
        Ok(Request::post(DATA_PATH, body)
            .timeout_ms(ENTRIES_TIMEOUT_MS)
            .response_cap(ENTRIES_CAP)
            .ndjson(ENTRIES_MAX_LINES)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        match body {
            Value::Array(lines) => Ok(Admitted::Done(lines)),
            _ => Err(Refusal::new(INVALID, "The survey stream was unreadable.")),
        }
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// One correction or deletion of an existing entry, as the capture outbox
/// prepared it. It is never sent twice: the outbox decides any retry.
#[derive(Clone, Debug, PartialEq)]
pub struct Mutation {
    pub payload: Map<String, Value>,
}

impl Exchange for Mutation {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let operation = self.payload.get("operation").and_then(Value::as_str);
        if !matches!(operation, Some("update" | "delete")) {
            return Err(input_invalid(
                "A survey mutation updates or deletes one entry.",
            ));
        }
        let request = Request {
            method: Method::Put,
            ..Request::post(MUTATE_PATH, Value::Object(self.payload.clone()))
        }
        .timeout_ms(MUTATE_TIMEOUT_MS)
        .response_cap(MUTATE_CAP);
        Ok(request.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(data(&body).clone()))
    }

    fn project(&self) -> Option<&str> {
        self.payload
            .get("eds_project_id")
            .or_else(|| self.payload.get("project_id"))
            .and_then(Value::as_str)
    }
}

/// Record entries a person discarded from the outbox, for the dead-letter
/// audit.
#[derive(Clone, Debug, PartialEq)]
pub struct Discarded {
    pub project: String,
    pub entries: Vec<Value>,
}

impl Exchange for Discarded {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let project = project_reference(&self.project)?;
        if self.entries.is_empty() || self.entries.len() > DISCARD_MAX_ENTRIES {
            return Err(input_invalid("A discard audit names 1 to 5,000 entries."));
        }
        let body = json!({"action": "submit_client_discarded", "project_id": project,
            "entries": self.entries});
        Ok(Request::post(DEADLETTER_PATH, body).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(data(&body).clone()))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// How a mutation ended, as the capture outbox reads it.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationOutcome {
    Success,
    /// Brain answered with a refusal, or nothing arrived (`network`, status 0).
    Response {
        status: u16,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(
            all(feature = "wasm", target_arch = "wasm32"),
            tsify(optional, type = "unknown")
        )]
        details: Option<Value>,
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        network: bool,
    },
}

/// The outbox's reading of one mutation's answer or refusal.
pub fn mutation_outcome(outcome: Result<Value, Refusal>) -> MutationOutcome {
    match outcome {
        Ok(_) => MutationOutcome::Success,
        // A mutation the Kernel would not send is the caller's error, never a
        // link to retry.
        Err(refusal) if refusal.code == INPUT_INVALID => MutationOutcome::Response {
            status: 400,
            network: false,
            details: None,
        },
        Err(refusal) => MutationOutcome::Response {
            status: refusal.status.unwrap_or(0),
            network: refusal.status.is_none(),
            details: refusal.details,
        },
    }
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{mutation_outcome, Discarded, Entries, Mutation, SurveyFilter};
    use crate::gateway_exchange::browser::{attempt, run, typed, GatewayHost};
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    /// One form's entries as Brain streams them: every line in order, the
    /// summary last.
    #[wasm_bindgen(js_name = surveyEntriesRead, unchecked_return_type = "Record<string, unknown>[]")]
    pub async fn survey_entries_read(
        host: GatewayHost,
        project: String,
        form: String,
        #[wasm_bindgen(unchecked_param_type = "SurveyFilter")] filter: JsValue,
        updated_after: Option<String>,
        include_tombstones: bool,
    ) -> Result<JsValue, JsValue> {
        let filter: SurveyFilter = if filter.is_undefined() || filter.is_null() {
            SurveyFilter::default()
        } else {
            tsify::serde_wasm_bindgen::from_value(filter).map_err(unreadable)?
        };
        let call = Entries {
            project,
            form,
            filter,
            updated_after,
            include_tombstones,
        };
        typed(&run(&host, call).await?)
    }

    /// Send one outbox correction or deletion once; answers how it ended.
    #[wasm_bindgen(js_name = surveyEntryMutation, unchecked_return_type = "MutationOutcome")]
    pub async fn survey_entry_mutation(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] payload: JsValue,
    ) -> Result<JsValue, JsValue> {
        let payload: Map<String, Value> =
            tsify::serde_wasm_bindgen::from_value(payload).map_err(unreadable)?;
        typed(&mutation_outcome(
            attempt(&host, Mutation { payload }).await?,
        ))
    }

    /// Record entries a person discarded, for the dead-letter audit.
    #[wasm_bindgen(js_name = surveyDiscardAudit, unchecked_return_type = "Record<string, unknown>")]
    pub async fn survey_discard_audit(
        host: GatewayHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "unknown[]")] entries: JsValue,
    ) -> Result<JsValue, JsValue> {
        let entries: Vec<Value> =
            tsify::serde_wasm_bindgen::from_value(entries).map_err(unreadable)?;
        typed(&run(&host, Discarded { project, entries }).await?)
    }
}

#[cfg(test)]
#[path = "survey_tests.rs"]
mod tests;
