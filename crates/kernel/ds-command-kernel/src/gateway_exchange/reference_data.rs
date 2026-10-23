//! A project's geographic reference datasets as ds-brain distributes them
//! (`POST /api/v1/data-distribution`): the governed catalogue, its
//! reconciliation and bundle publication, and bounded cloud pages of one
//! dataset version. A catalogue row that does not read as a dataset refuses
//! the catalogue; a page is admitted only when it answers exactly the
//! dataset, version, offset and area asked for and its features match their
//! digest and count.

use ds_gateway_client::Operation;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

const CONTROL_TIMEOUT_MS: u64 = 150_000;
const PAGE_TIMEOUT_MS: u64 = 120_000;
pub const PAGE_ROWS: u64 = 1000;
/// The most feature bytes one cloud query reads, across its pages.
pub const QUERY_BYTES: usize = 16 * 1024 * 1024;
/// The most features one cloud query reads.
pub const QUERY_ROWS: u64 = 10_000;
const CATALOG_CAP: u64 = 8 * 1024 * 1024;
const PAGE_CAP: u64 = 40 * 1024 * 1024;
pub const INVALID: &str = "reference_answer_invalid";
pub const INPUT_INVALID: &str = "reference_input_invalid";

fn invalid(what: &str) -> Refusal {
    Refusal::new(INVALID, format!("The geographic data service answered an invalid {what}."))
}

fn hex64(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|text| text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

fn positive(value: Option<&Value>) -> bool {
    value.and_then(Value::as_u64).is_some_and(|number| number > 0)
}

/// Whether one catalogue row reads as a dataset: its identity, an active
/// BigQuery source, and either no desktop bundle or a complete one served
/// from storage.
pub fn dataset(row: &Value) -> bool {
    let Some(fields) = row.as_object() else { return false };
    let text = |key: &str| fields.get(key).and_then(Value::as_str);
    let bundle = [
        "download_url", "bundle_sha256", "download_bytes", "expanded_bytes", "expanded_sha256",
        "bundle_features", "bundle_built_at",
    ];
    let bundled = bundle.iter().any(|key| fields.contains_key(*key));
    let bundle_ok = !bundled
        || (text("download_url")
            .and_then(|url| url::Url::parse(url).ok())
            .is_some_and(|url| url.scheme() == "https" && url.host_str() == Some("storage.googleapis.com"))
            && hex64(fields.get("bundle_sha256"))
            && hex64(fields.get("expanded_sha256"))
            && ["download_bytes", "expanded_bytes", "bundle_features", "bundle_built_at"]
                .iter()
                .all(|key| positive(fields.get(*key))));
    hex64(fields.get("id"))
        && (hex64(fields.get("version")) || text("unavailable").is_some())
        && text("label").is_some()
        && fields.get("active") == Some(&Value::Bool(true))
        && text("kind") == Some("bigquery")
        && fields.get("source_modified_at").is_none_or(|value| positive(Some(value)))
        && bundle_ok
}

fn catalogue(body: &Value) -> Result<Vec<Value>, Refusal> {
    let rows = data(body).as_array().ok_or_else(|| invalid("catalogue"))?;
    if !rows.iter().all(dataset) {
        return Err(invalid("catalogue row"));
    }
    Ok(rows.clone())
}

fn control(
    action: &str,
    project: &str,
    extra: Map<String, Value>,
    timeout_ms: u64,
    cap: u64,
) -> Result<Plan, Refusal> {
    let mut body = json!({"action": action, "project_id": project_reference(project)?});
    body.as_object_mut().expect("an object").extend(extra);
    Ok(
        Request::generated(Operation::DataDistributionAction, Some(body))?
            .timeout_ms(timeout_ms)
            .response_cap(cap)
            .into(),
    )
}

fn resource_id(id: &str) -> Result<Map<String, Value>, Refusal> {
    if id.trim().is_empty() || id.len() > 128 {
        return Err(Refusal::new(INPUT_INVALID, "A request names one dataset."));
    }
    let mut extra = Map::new();
    extra.insert("resource_id".into(), id.into());
    Ok(extra)
}

/// The project's governed dataset catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalogue {
    pub project: String,
}

impl Exchange for Catalogue {
    type Output = Vec<Value>;
    fn plan(&self) -> Result<Plan, Refusal> {
        control("list_datasets", &self.project, Map::new(), CONTROL_TIMEOUT_MS, CATALOG_CAP)
    }
    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        catalogue(&body).map(Admitted::Done)
    }
    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// Reconcile the catalogue with its sources and answer it (global tiles managers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reconcile {
    pub project: String,
}

impl Exchange for Reconcile {
    type Output = Vec<Value>;
    fn plan(&self) -> Result<Plan, Refusal> {
        control("reconcile_datasets", &self.project, Map::new(), CONTROL_TIMEOUT_MS, CATALOG_CAP)
    }
    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        catalogue(&body).map(Admitted::Done)
    }
    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// Start one dataset's desktop bundle publication, or read its durable job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Publication {
    pub project: String,
    pub resource_id: String,
    /// `true` starts the job; `false` only reads it.
    pub start: bool,
}

impl Exchange for Publication {
    type Output = Value;
    fn plan(&self) -> Result<Plan, Refusal> {
        let action = if self.start { "publish_dataset" } else { "publication_status" };
        control(action, &self.project, resource_id(&self.resource_id)?, CONTROL_TIMEOUT_MS, CATALOG_CAP)
    }
    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(data(&body).clone()))
    }
    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// One admitted cloud page.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Page {
    pub features: Vec<Value>,
    pub next_offset: Option<u64>,
    pub bytes: u64,
}

/// One bounded page of one dataset version, optionally within an area.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryPage {
    pub project: String,
    pub resource_id: String,
    pub version: String,
    pub offset: u64,
    pub area: Option<Value>,
}

/// JSON with object keys sorted, for comparing an echoed area.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            Value::Object(keys.into_iter().map(|key| (key.clone(), canonical(&map[key]))).collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

impl Exchange for QueryPage {
    type Output = Page;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut extra = resource_id(&self.resource_id)?;
        extra.insert("resource_version".into(), self.version.clone().into());
        extra.insert("offset".into(), self.offset.into());
        if let Some(area) = &self.area {
            extra.insert("area".into(), area.clone());
        }
        control("query_dataset", &self.project, extra, PAGE_TIMEOUT_MS, PAGE_CAP)
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let page = data(&body);
        let text = |key: &str| page.get(key).and_then(Value::as_str);
        let echoed_area = canonical(page.get("area").unwrap_or(&Value::Null));
        let asked_area = canonical(self.area.as_ref().unwrap_or(&Value::Null));
        if text("resource_id") != Some(self.resource_id.as_str())
            || text("version") != Some(self.version.as_str())
            || page.get("offset").and_then(Value::as_u64) != Some(self.offset)
            || echoed_area != asked_area
        {
            return Err(Refusal::new(INVALID, "Reference page identity or query scope mismatch."));
        }
        let feature_json = text("feature_json").ok_or_else(|| invalid("page"))?;
        let digest = format!("{:x}", Sha256::digest(feature_json.as_bytes()));
        if feature_json.len() > QUERY_BYTES || text("sha256") != Some(digest.as_str()) {
            return Err(Refusal::new(INVALID, "Reference page integrity check failed."));
        }
        let features: Vec<Value> = serde_json::from_str(feature_json).map_err(|_| invalid("page"))?;
        let next_offset = page.get("next_offset").and_then(Value::as_u64);
        let complete = features.len() as u64 <= PAGE_ROWS
            && page.get("feature_count").and_then(Value::as_u64) == Some(features.len() as u64)
            && features.iter().all(|feature| {
                feature.get("type").and_then(Value::as_str) == Some("Feature")
                    && feature.get("properties").is_some_and(Value::is_object)
                    && feature.get("geometry").is_some_and(|geometry| geometry.is_null() || geometry.is_object())
            })
            && match page.get("next_offset") {
                None | Some(Value::Null) => true,
                Some(_) => next_offset == Some(self.offset + PAGE_ROWS) && features.len() as u64 == PAGE_ROWS,
            };
        if !complete {
            return Err(Refusal::new(INVALID, "Reference page is incomplete or out of order."));
        }
        Ok(Admitted::Done(Page {
            bytes: feature_json.len() as u64,
            features,
            next_offset,
        }))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// The catalogue this device holds for a project, as it was admitted.
#[cfg(feature = "durable-store")]
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct HeldCatalogue {
    pub resources: Vec<Value>,
    pub saved_at_ms: u64,
}

/// The catalogue this device holds for one project, if any. Held per account
/// and project; another account's never reads.
#[cfg(feature = "durable-store")]
pub async fn held_catalogue(
    edge: &impl super::retained::Edge,
    project: &str,
) -> Result<Option<HeldCatalogue>, Refusal> {
    project_reference(project)?;
    let kind = crate::retained_reads::Kind::ReferenceCatalogue;
    let observed = super::retained::read::<Vec<Value>>(edge, kind, Some(project), "").await?;
    Ok(observed.held.map(|held| HeldCatalogue {
        resources: held.value,
        saved_at_ms: held.saved_at_ms,
    }))
}

/// Hold a catalogue Brain answered for one project, admitted again here, for
/// the next offline read. A store that cannot keep it never hides it.
#[cfg(feature = "durable-store")]
pub async fn retain_catalogue(
    edge: &impl super::retained::Edge,
    project: &str,
    rows: Vec<Value>,
) -> Result<HeldCatalogue, Refusal> {
    project_reference(project)?;
    let rows = catalogue(&json!({"success": true, "data": rows}))?;
    let kind = crate::retained_reads::Kind::ReferenceCatalogue;
    if let Ok(observed) = super::retained::read::<Vec<Value>>(edge, kind, Some(project), "").await {
        let owner = observed.owner.as_deref();
        let _ = super::retained::write(edge, owner, kind, Some(project), "", &rows, None).await;
    }
    Ok(HeldCatalogue {
        resources: rows,
        saved_at_ms: edge.now_ms(),
    })
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{Catalogue, Publication, QueryPage, Reconcile, QUERY_BYTES, QUERY_ROWS};
    use crate::gateway_exchange::browser::{rejection, run, typed, GatewayHost};
    use crate::gateway_exchange::Refusal;
    use gloo_timers::future::TimeoutFuture;
    use serde_json::{json, Value};
    use wasm_bindgen::prelude::*;
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::retained::browser::BrowserEdge;

    /// The catalogue this device holds for a project, or none.
    #[wasm_bindgen(js_name = referenceCatalogueHeld, unchecked_return_type = "{ resources: Record<string, unknown>[]; saved_at_ms: number } | null")]
    pub async fn reference_catalogue_held(
        host: GatewayHost,
        store: StoreHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let held = super::held_catalogue(&edge, &project).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// Hold a catalogue Brain answered, admitted again, for offline reads.
    #[wasm_bindgen(js_name = referenceCatalogueRetain, unchecked_return_type = "{ resources: Record<string, unknown>[]; saved_at_ms: number }")]
    pub async fn reference_catalogue_retain(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>[]")] rows: JsValue,
    ) -> Result<JsValue, JsValue> {
        let rows: Vec<Value> = tsify::serde_wasm_bindgen::from_value(rows)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let edge = BrowserEdge::new(host, store);
        let held = super::retain_catalogue(&edge, &project, rows).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// How long a bundle publication may run before the page stops waiting.
    const PUBLISH_WAIT_MS: f64 = 30.0 * 60_000.0;

    /// The project's governed dataset catalogue.
    #[wasm_bindgen(js_name = referenceCatalogue, unchecked_return_type = "Record<string, unknown>[]")]
    pub async fn reference_catalogue(host: GatewayHost, project: String) -> Result<JsValue, JsValue> {
        typed(&run(&host, Catalogue { project }).await?)
    }

    /// Reconcile the catalogue with its sources and answer it.
    #[wasm_bindgen(js_name = referenceReconcile, unchecked_return_type = "Record<string, unknown>[]")]
    pub async fn reference_reconcile(host: GatewayHost, project: String) -> Result<JsValue, JsValue> {
        typed(&run(&host, Reconcile { project }).await?)
    }

    /// One dataset's publication job, read without starting it.
    #[wasm_bindgen(js_name = referencePublicationStatus, unchecked_return_type = "Record<string, unknown>")]
    pub async fn reference_publication_status(host: GatewayHost, project: String, resource_id: String) -> Result<JsValue, JsValue> {
        typed(&run(&host, Publication { project, resource_id, start: false }).await?)
    }

    /// Publish one dataset's desktop bundle and wait for its job to finish
    /// (at most 30 minutes); answers the reconciled catalogue.
    #[wasm_bindgen(js_name = referencePublish, unchecked_return_type = "Record<string, unknown>[]")]
    pub async fn reference_publish(host: GatewayHost, project: String, resource_id: String) -> Result<JsValue, JsValue> {
        run(&host, Publication { project: project.clone(), resource_id: resource_id.clone(), start: true }).await?;
        let deadline = js_sys::Date::now() + PUBLISH_WAIT_MS;
        loop {
            let status = run(&host, Publication { project: project.clone(), resource_id: resource_id.clone(), start: false }).await?;
            match status.get("status").and_then(Value::as_str) {
                Some("completed") => break,
                Some("failed") => {
                    let message = ["last_error", "message"].iter().find_map(|key| status.get(*key).and_then(Value::as_str))
                        .unwrap_or("Dataset bundle publication failed.");
                    return Err(rejection(&Refusal::new("reference_publication_failed", message)));
                }
                _ if js_sys::Date::now() >= deadline => {
                    return Err(rejection(&Refusal::new("reference_publication_running", "Dataset bundle publication is still running after 30 minutes.")));
                }
                _ => TimeoutFuture::new(2_000).await,
            }
        }
        typed(&run(&host, Reconcile { project }).await?)
    }

    /// A cloud query of one dataset version within an area: bounded pages,
    /// each admitted against what was asked, at most 10 000 features or
    /// 16 MiB, with `truncated` when the bound cut it.
    #[wasm_bindgen(js_name = referenceQueryCloud, unchecked_return_type = "{ features: unknown[]; truncated: boolean }")]
    pub async fn reference_query_cloud(
        host: GatewayHost,
        project: String,
        resource_id: String,
        version: String,
        #[wasm_bindgen(unchecked_param_type = "unknown")] area: JsValue,
    ) -> Result<JsValue, JsValue> {
        let area: Option<Value> = if area.is_undefined() || area.is_null() {
            None
        } else {
            Some(tsify::serde_wasm_bindgen::from_value(area).map_err(|error| js_sys::Error::new(&error.to_string()))?)
        };
        let mut features = Vec::new();
        let mut bytes = 0usize;
        let mut offset = 0u64;
        while offset < QUERY_ROWS {
            let call = QueryPage { project: project.clone(), resource_id: resource_id.clone(), version: version.clone(), offset, area: area.clone() };
            let page = run(&host, call).await?;
            bytes += page.bytes as usize;
            if bytes > QUERY_BYTES {
                return typed(&json!({ "features": features, "truncated": true }));
            }
            features.extend(page.features);
            match page.next_offset {
                None => return typed(&json!({ "features": features, "truncated": false })),
                Some(next) => offset = next,
            }
        }
        typed(&json!({ "features": features, "truncated": true }))
    }
}

#[cfg(test)]
#[path = "reference_data_tests.rs"]
mod tests;
