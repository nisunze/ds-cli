//! Survey entries over the gateway. A page holds each form it reads the way
//! the core does (`survey::hold`): the kernel decides reuse | delta |
//! replace over what this device holds, Brain streams `query_entries` when
//! asked, and the verified rows are kept on the page's Kernel store so the
//! next open, online or not, paints at once. Also one entry correction or
//! deletion from the capture outbox, and the audit of entries a person chose
//! to discard. Brain authorizes each project and form.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, project_reference, Admitted, Exchange, Method, Plan, Refusal, Request};
use crate::survey::hold::{self, Filter, Manifest};

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
pub const INPUT_INVALID: &str = "survey_input_invalid";
pub const INVALID: &str = "survey_answer_invalid";
/// `local` and nothing covering is held: the core's own code.
pub const NOT_HELD: &str = "survey_hold_not_held";

fn input_invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

/// The `query_entries` request the hold decided, streamed as NDJSON.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    pub project: String,
    pub body: Value,
}

impl Exchange for Query {
    /// Every streamed line in order: the entries, then Brain's summary.
    type Output = Vec<Value>;

    fn plan(&self) -> Result<Plan, Refusal> {
        project_reference(&self.project)?;
        if self.body["action"] != hold::QUERY_ACTION || self.body["eds_project_id"] != self.project.as_str() {
            return Err(input_invalid("A survey read is one query_entries of its project."));
        }
        Ok(Request::post(DATA_PATH, self.body.clone())
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

/// One read of one form, as the window asks it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HoldRequest {
    pub project: String,
    pub form: String,
    #[serde(default)]
    pub filter: Filter,
    /// `auto` (the default), `delta`, `full` or `local`.
    #[serde(default)]
    pub refresh: Option<String>,
    /// Only rows changed after this instant, from what is held.
    #[serde(default, rename = "updatedAfter")]
    pub updated_after: Option<String>,
}

/// Who answered: the held copy as it was, or a refresh of it.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Held,
    Delta,
    Replace,
}

/// The rows of one form in the window's view, and what is held of it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct HoldAnswer {
    pub source: Source,
    pub held: Manifest,
    pub rows: Vec<Value>,
}

#[cfg(feature = "durable-store")]
/// What the page's store keeps of one form.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct HeldForm {
    manifest: Manifest,
    rows: Vec<Value>,
}

#[cfg(feature = "durable-store")]
fn refresh_policy(refresh: Option<&str>) -> Result<hold::Refresh, Refusal> {
    Ok(match refresh.unwrap_or("auto") {
        "auto" => hold::Refresh::Auto {
            max_age_seconds: hold::DEFAULT_MAX_AGE_SECONDS,
        },
        "delta" => hold::Refresh::Delta,
        "full" => hold::Refresh::Full,
        "local" => hold::Refresh::Local,
        _ => return Err(input_invalid("A survey read refreshes auto, delta, full or local.")),
    })
}

#[cfg(feature = "durable-store")]
fn instant(now_ms: u64) -> Result<time::OffsetDateTime, Refusal> {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(now_ms) * 1_000_000)
        .map_err(|_| Refusal::new(INVALID, "The host clock is out of range."))
}

/// The held rows as NDJSON text, as the core's rows file digests them.
#[cfg(feature = "durable-store")]
fn rows_sha256(rows: &[Value]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for row in rows {
        digest.update(row.to_string().as_bytes());
        digest.update(b"\n");
    }
    format!("{:x}", digest.finalize())
}

#[cfg(feature = "durable-store")]
fn stream(lines: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines {
        out.extend_from_slice(line.to_string().as_bytes());
        out.push(b'\n');
    }
    out
}

/// Answer one read of one form: reuse, refresh or read it, as the kernel
/// decides over what this device holds, then keep the result. A refresh that
/// fails leaves what is held as it was.
#[cfg(feature = "durable-store")]
pub async fn hold_read(
    edge: &impl super::retained::Edge,
    request: HoldRequest,
) -> Result<HoldAnswer, Refusal> {
    use hold::{ReadMode, View};
    let project = project_reference(&request.project)?.to_owned();
    let form = request.form;
    if !hold::valid_form_slug(&form) {
        return Err(input_invalid("A survey read names one exact form slug."));
    }
    let refresh = refresh_policy(request.refresh.as_deref())?;
    let wanted = request.filter.canonical().map_err(input_invalid)?;
    let now = instant(edge.now_ms())?;
    let kind = crate::retained_reads::Kind::SurveyHold;
    // `owner` is the store's answer; `None` when the store could not read.
    let (owner, held) =
        match super::retained::read::<HeldForm>(edge, kind, Some(&project), &form).await {
            Ok(observed) => (Some(observed.owner), observed.held.map(|held| held.value)),
            Err(_) => (None, None),
        };
    let decision = hold::decide(
        &project,
        &form,
        held.as_ref().map(|held| &held.manifest),
        &wanted,
        refresh,
        now,
    )
    .map_err(|error| Refusal::new(INVALID, error))?;
    let (source, kept) = match decision {
        hold::Decision::NotHeld => {
            return Err(Refusal::new(NOT_HELD, "Nothing of this form is held here."))
        }
        hold::Decision::Reuse => (
            Source::Held,
            held.ok_or_else(|| Refusal::new(NOT_HELD, "The held copy vanished."))?,
        ),
        hold::Decision::Query { mode, body } => {
            let lines = edge
                .fetch(Query {
                    project: project.clone(),
                    body,
                })
                .await?;
            // A delta merges onto the held copy under its own filter; a
            // replace is the requested filter from nothing.
            let applied = match (mode, held) {
                (ReadMode::Delta, Some(HeldForm { manifest, rows })) => {
                    let filter = manifest.filter.clone();
                    hold::apply(&project, &form, Some((&manifest, rows)), &filter, mode, &stream(&lines), now)
                }
                (_, held) => {
                    let base = held.as_ref().map(|held| (&held.manifest, Vec::new()));
                    hold::apply(&project, &form, base, &wanted, mode, &stream(&lines), now)
                }
            }
            .map_err(|error| Refusal::new(INVALID, error))?;
            let mut manifest = applied.manifest;
            manifest.rows_sha256 = rows_sha256(&applied.rows);
            let kept = HeldForm {
                manifest,
                rows: applied.rows,
            };
            if let Some(owner) = &owner {
                let _ = super::retained::write(edge, owner.as_deref(), kind, Some(&project), &form, &kept, None)
                    .await;
            }
            let source = match mode {
                ReadMode::Delta => Source::Delta,
                ReadMode::Replace => Source::Replace,
            };
            (source, kept)
        }
    };
    let view = View {
        bbox: wanted.bbox,
        updated_after: request.updated_after,
    };
    let rows = kept.rows.into_iter().filter(|row| view.admits(row)).collect();
    Ok(HoldAnswer {
        source,
        held: kept.manifest,
        rows,
    })
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
        // A create is never a mutation: it is a direct Firestore write as its
        // author ([`super::survey_create`]); Brain never creates.
        let operation = self.payload.get("operation").and_then(Value::as_str);
        if !matches!(operation, Some("update" | "delete" | "restore")) {
            return Err(input_invalid(
                "A survey mutation updates, deletes or restores one entry.",
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
    use super::{hold_read, mutation_outcome, Discarded, HoldRequest, Mutation};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{attempt, rejection, run, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use serde_json::{Map, Value};
    use wasm_bindgen::prelude::*;

    fn unreadable(error: impl std::fmt::Display) -> JsValue {
        js_sys::Error::new(&error.to_string()).into()
    }

    /// One form's rows in the window's view, from what this device holds,
    /// refreshed as the kernel decides; `local` never reaches the cloud.
    #[wasm_bindgen(
        js_name = surveyHoldRead,
        unchecked_return_type = "{ source: 'held' | 'delta' | 'replace'; held: Record<string, unknown>; rows: Record<string, unknown>[] }"
    )]
    pub async fn survey_hold_read(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(
            unchecked_param_type = "{ project: string; form: string; filter?: { bbox?: [number, number, number, number]; admin_boundary?: { code: string; name?: string }; boundary?: Record<string, unknown>; date_from?: string; date_to?: string; surveyors?: string[] }; refresh?: 'auto' | 'delta' | 'full' | 'local'; updatedAfter?: string }"
        )]
        request: JsValue,
    ) -> Result<JsValue, JsValue> {
        let request: HoldRequest =
            tsify::serde_wasm_bindgen::from_value(request).map_err(unreadable)?;
        let edge = BrowserEdge::new(host, store);
        typed(&hold_read(&edge, request).await.map_err(|refusal| rejection(&refusal))?)
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
