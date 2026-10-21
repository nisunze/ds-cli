//! A project's documents as ds-brain's catalogue serves them (`POST
//! /api/v1/assets`): pages of rows, the declared folders, one row, one signed
//! read, a classification and a link change. A row reaches the page only when
//! it reads as the Kernel's asset model ([`crate::assets::Asset::from_wire`]);
//! one that does not is dropped and counted, never half-shown. The row itself
//! travels as ds-brain sent it. The index view of the same door is
//! [`super::assets_index`].

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};
use crate::assets::{Asset, Folder};

const PATH: &str = "/api/v1/assets";
/// A selected document can arrive over a slow workstation connection.
const READ_TIMEOUT_MS: u64 = 180_000;
const PAGE_CAP: u64 = 8 * 1024 * 1024;
const ROW_CAP: u64 = 1024 * 1024;
pub const INVALID: &str = "ASSETS_INVALID_RESPONSE";
pub const INPUT_INVALID: &str = "assets_input_invalid";

fn invalid(what: &str) -> Refusal {
    Refusal::new(INVALID, format!("The assets service returned an invalid {what}."))
}

fn input(what: &str) -> Refusal {
    Refusal::new(INPUT_INVALID, format!("An assets request names one valid {what}."))
}

/// A catalogue row exactly as served, once it reads as the Kernel's model.
fn row(value: &Value) -> Option<Value> {
    Asset::from_wire(value).map(|_| value.clone())
}

fn required_row(value: Option<&Value>, what: &str) -> Result<Value, Refusal> {
    value.and_then(row).ok_or_else(|| invalid(what))
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn asset_id(id: &str) -> Result<&str, Refusal> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.len() > 512 || trimmed.chars().any(char::is_control) {
        return Err(input("asset"));
    }
    Ok(trimmed)
}

fn body(action: &str, project: &str, payload: Map<String, Value>) -> Result<Value, Refusal> {
    let mut body = json!({"action": action, "ds_project": project_reference(project)?});
    body.as_object_mut().expect("an object").extend(payload);
    Ok(body)
}

fn read(action: &str, project: &str, payload: Map<String, Value>, cap: u64) -> Result<Plan, Refusal> {
    Ok(Request::post(PATH, body(action, project, payload)?)
        .timeout_ms(READ_TIMEOUT_MS)
        .response_cap(cap)
        .into())
}

fn write(action: &str, project: &str, payload: Map<String, Value>) -> Result<Plan, Refusal> {
    Ok(Request::post(PATH, body(action, project, payload)?)
        .response_cap(ROW_CAP)
        .into())
}

/// What a catalogue page asks for; every member is optional.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub folder_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub sensitivity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub since: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub cursor: Option<String>,
}

/// One catalogue page; `dropped` counts rows that did not read as assets.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Page {
    pub assets: Vec<Value>,
    pub has_more: bool,
    pub limit: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub scanned: u64,
    pub truncated: bool,
    pub dropped: u64,
}

/// One page of the project's catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List {
    pub project: String,
    pub query: ListQuery,
}

impl Exchange for List {
    type Output = Page;

    fn plan(&self) -> Result<Plan, Refusal> {
        let query = serde_json::to_value(&self.query).map_err(|_| input("query"))?;
        let mut payload = Map::new();
        for (name, value) in query.as_object().into_iter().flatten() {
            let empty = value.as_str().is_some_and(|text| text.trim().is_empty());
            if !empty && *value != json!(0) {
                payload.insert(name.clone(), value.clone());
            }
        }
        read("list", &self.project, payload, PAGE_CAP)
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let raw = answer
            .get("assets")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let assets: Vec<Value> = raw.iter().filter_map(row).collect();
        let number = |key: &str| answer.get(key).and_then(Value::as_u64);
        Ok(Admitted::Done(Page {
            dropped: (raw.len() - assets.len()) as u64,
            has_more: answer.get("has_more") == Some(&Value::Bool(true)),
            limit: number("limit").unwrap_or(assets.len() as u64),
            next_cursor: text(answer.get("next_cursor")),
            scanned: number("scanned").unwrap_or(raw.len() as u64),
            truncated: answer.get("truncated") == Some(&Value::Bool(true)),
            assets,
        }))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// The project's declared folders with this caller's counts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    pub project: String,
}

impl Exchange for Tree {
    type Output = Vec<Value>;

    fn plan(&self) -> Result<Plan, Refusal> {
        read("tree", &self.project, Map::new(), PAGE_CAP)
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let folders = data(&body)
            .get("folders")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|folder| Folder::from_wire(folder).is_some())
            .cloned()
            .collect();
        Ok(Admitted::Done(folders))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// One row of the catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Get {
    pub project: String,
    pub asset_id: String,
}

impl Exchange for Get {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut payload = Map::new();
        payload.insert("asset_id".into(), asset_id(&self.asset_id)?.into());
        read("get", &self.project, payload, ROW_CAP)
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        required_row(Some(data(&body)), "asset").map(Admitted::Done)
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// One signed read: the row and a short-lived URL to use once.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Download {
    pub asset: Value,
    pub download_url: String,
    pub expires_at: String,
}

/// Mint one signed read; ds-brain audits restricted and confidential reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedRead {
    pub project: String,
    pub asset_id: String,
    /// `ui`, `cli` or `mcp`: the surface the read is audited under.
    pub surface: Option<String>,
}

impl Exchange for SignedRead {
    type Output = Download;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut payload = Map::new();
        payload.insert("asset_id".into(), asset_id(&self.asset_id)?.into());
        if let Some(surface) = &self.surface {
            if !matches!(surface.as_str(), "ui" | "cli" | "mcp") {
                return Err(input("read surface"));
            }
            payload.insert("surface".into(), surface.clone().into());
        }
        read("download", &self.project, payload, ROW_CAP)
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let download_url = text(answer.get("download_url"))
            .filter(|url| url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "https" | "http")))
            .ok_or_else(|| invalid("download"))?;
        Ok(Admitted::Done(Download {
            asset: required_row(answer.get("asset"), "download")?,
            download_url,
            expires_at: text(answer.get("expires_at")).ok_or_else(|| invalid("download"))?,
        }))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// A classification's receipt.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Classified {
    pub asset: Value,
    pub audit_id: String,
    pub changed: Vec<String>,
}

/// Classify one row, fenced by the version the caller holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Classify {
    pub project: String,
    pub asset_id: String,
    /// `kind`, `status`, `owner`, `folder_id`, `sensitivity`, `reason`,
    /// `document` and the required `expected_version`, as ds-brain names them.
    pub classify: Map<String, Value>,
}

impl Exchange for Classify {
    type Output = Classified;

    fn plan(&self) -> Result<Plan, Refusal> {
        if !self.classify.get("expected_version").is_some_and(Value::is_u64) {
            return Err(input("expected version"));
        }
        let classify: Map<String, Value> = self
            .classify
            .iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        let mut payload = Map::new();
        payload.insert("asset_id".into(), asset_id(&self.asset_id)?.into());
        payload.insert("classify".into(), Value::Object(classify));
        write("classify", &self.project, payload)
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        Ok(Admitted::Done(Classified {
            asset: required_row(answer.get("asset"), "classification")?,
            audit_id: text(answer.get("audit_id")).ok_or_else(|| invalid("classification"))?,
            changed: answer
                .get("changed")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|name| text(Some(name)))
                .collect(),
        }))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// Link one row to a Project Work task or record, a DS object or a tag, or
/// remove that link.
#[derive(Clone, Debug, PartialEq)]
pub struct Relink {
    pub project: String,
    pub asset_id: String,
    pub link: Value,
    pub detach: bool,
}

/// The one shape a link travels in, or `None`.
fn link(value: &Value) -> Option<Value> {
    let field = |key: &str| text(value.get(key));
    Some(match value.get("kind").and_then(Value::as_str)? {
        "tag" => json!({"kind": "tag", "definition_id": field("definition_id")?, "value": field("value")?}),
        kind @ ("pm_task" | "pm_record") => json!({"kind": kind, "id": field("id")?}),
        "ds_object" => json!({"kind": "ds_object", "object_type": field("object_type")?, "entity_id": field("entity_id")?}),
        _ => return None,
    })
}

impl Exchange for Relink {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut payload = Map::new();
        payload.insert("asset_id".into(), asset_id(&self.asset_id)?.into());
        payload.insert("link".into(), link(&self.link).ok_or_else(|| input("link"))?);
        write(if self.detach { "detach" } else { "attach" }, &self.project, payload)
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        required_row(Some(answer.get("asset").unwrap_or(answer)), "asset").map(Admitted::Done)
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// The asset is gone, or this caller may no longer read it.
pub fn revoked(refusal: &Refusal) -> bool {
    matches!(refusal.status, Some(403 | 404))
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use js_sys::{Reflect, Uint8Array};
    use serde_json::{json, Value};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    use super::{Classify, Get, List, ListQuery, Relink, SignedRead, Tree};
    use crate::assets::index_read::{MutationReceipt, Scope};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::assets_index::timeline::{self, acquire, begin, Begun, Rooms, TimelinePage};
    use crate::gateway_exchange::assets_index::{predates_index, IndexGet, IndexQuery, RowAnswer};
    use crate::gateway_exchange::browser::{attempt, rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use crate::gateway_exchange::retained::Edge;
    use crate::gateway_exchange::storage::fetch_verified;
    use crate::gateway_exchange::Refusal;

    #[wasm_bindgen(typescript_custom_section)]
    const TYPES: &'static str = r#"
/** A catalogue page; rows travel as ds-brain served them (`AssetRow`). */
export interface AssetsPage<Row = Record<string, unknown>> { assets: Row[]; has_more: boolean; limit: number; next_cursor?: string; scanned: number; truncated: boolean; dropped: number }
/** One signed read; use the URL now and forget it. */
export interface AssetsDownload<Row = Record<string, unknown>> { asset: Row; download_url: string; expires_at: string }
export interface AssetsClassified<Row = Record<string, unknown>> { asset: Row; audit_id: string; changed: string[] }
/** One index page for the timeline (`cached`: this device's copy), or a ds-brain without the index. */
export type AssetsIndexAnswer =
    | { status: 'served'; rows: IndexRow[]; more: boolean; cursor?: string; meta: IndexMeta | null; cached?: boolean }
    | { status: 'unavailable'; rows: IndexRow[] };
/** Whose index a page belongs to: the signed-in account, its lane and authority, one project. */
export interface AssetsScope { uid: string; email: string; lane: string; audience: string; project: string }
"#;

    /// A refusal as the page's `Error`, marked `revoked` when the asset is gone
    /// or this caller may no longer read it.
    fn refused(refusal: &Refusal) -> JsValue {
        let error = rejection(refusal);
        if super::revoked(refusal) {
            let _ = Reflect::set(&error, &JsValue::from_str("revoked"), &JsValue::TRUE);
        }
        error
    }

    async fn ask<C: crate::gateway_exchange::Exchange>(host: &GatewayHost, call: C) -> Result<C::Output, JsValue> {
        attempt(host, call).await?.map_err(|refusal| refused(&refusal))
    }

    fn input<T: serde::de::DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
        tsify::serde_wasm_bindgen::from_value(value)
            .map_err(|error| js_sys::Error::new(&error.to_string()).into())
    }

    /// One page of the project's catalogue.
    #[wasm_bindgen(js_name = assetsList, unchecked_return_type = "AssetsPage")]
    pub async fn assets_list(
        host: GatewayHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "ListQuery | undefined")] query: JsValue,
    ) -> Result<JsValue, JsValue> {
        let query: ListQuery = if query.is_undefined() || query.is_null() { ListQuery::default() } else { input(query)? };
        typed(&ask(&host, List { project, query }).await?)
    }

    /// The project's declared folders (`AssetFolderRow`, as served).
    #[wasm_bindgen(js_name = assetsTree, unchecked_return_type = "Record<string, unknown>[]")]
    pub async fn assets_tree(host: GatewayHost, project: String) -> Result<JsValue, JsValue> {
        typed(&ask(&host, Tree { project }).await?)
    }

    /// One catalogue row (`AssetRow`, as served).
    #[wasm_bindgen(js_name = assetsGet, unchecked_return_type = "Record<string, unknown>")]
    pub async fn assets_get(host: GatewayHost, project: String, asset_id: String) -> Result<JsValue, JsValue> {
        typed(&ask(&host, Get { project, asset_id }).await?)
    }

    /// One signed read of one row.
    #[wasm_bindgen(js_name = assetsDownload, unchecked_return_type = "AssetsDownload")]
    pub async fn assets_download(
        host: GatewayHost,
        project: String,
        asset_id: String,
        surface: Option<String>,
    ) -> Result<JsValue, JsValue> {
        typed(&ask(&host, SignedRead { project, asset_id, surface }).await?)
    }

    /// One row's bytes, read once through a fresh signed URL and checked
    /// against the row's size (and its SHA-256 digest when it names one).
    #[wasm_bindgen(js_name = assetsBytes, unchecked_return_type = "{ asset: Record<string, unknown>; bytes: Uint8Array }")]
    pub async fn assets_bytes(
        host: GatewayHost,
        project: String,
        asset_id: String,
        max_bytes: f64,
    ) -> Result<JsValue, JsValue> {
        let download = ask(&host, SignedRead { project, asset_id, surface: Some("ui".into()) }).await?;
        let size = download.asset.get("bytes").and_then(Value::as_u64).unwrap_or(0);
        if max_bytes.is_finite() && size as f64 > max_bytes {
            return Err(rejection(&crate::gateway_exchange::Refusal::new(
                crate::gateway_exchange::RESPONSE_TOO_LARGE,
                "The document exceeds the requested byte bound.",
            )));
        }
        let digest = download
            .asset
            .get("digest")
            .and_then(Value::as_str)
            .map(|digest| digest.trim_start_matches("sha256:").to_ascii_lowercase())
            .filter(|digest| crate::report_export::exact_lower_sha256(digest));
        let bytes = match digest {
            Some(sha) => fetch_verified(&download.download_url, size, &sha).await,
            None => crate::gateway_exchange::storage::fetch_bounded(&download.download_url, size).await,
        }
        .map_err(|refusal| rejection(&refusal))?;
        let answer = typed(&json!({ "asset": download.asset }))?;
        Reflect::set(&answer, &JsValue::from_str("bytes"), &Uint8Array::from(&bytes[..]))?;
        Ok(answer)
    }

    /// Classify one row, fenced by `classify.expected_version`. The receipt
    /// of exactly that change marks this device's held index pages stale.
    #[wasm_bindgen(js_name = assetsClassify, unchecked_return_type = "AssetsClassified")]
    pub async fn assets_classify(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "AssetsScope")] scope: JsValue,
        asset_id: String,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] classify: JsValue,
    ) -> Result<JsValue, JsValue> {
        let scope: Scope = input(scope)?;
        let classify: serde_json::Map<String, Value> = input(classify)?;
        let expected_version = classify.get("expected_version").and_then(Value::as_u64).unwrap_or(0);
        let edge = BrowserEdge::new(host, store);
        let project = scope.project.clone();
        let receipt = edge
            .fetch(Classify { project, asset_id: asset_id.clone(), classify })
            .await
            .map_err(|refusal| refused(&refusal))?;
        let mutation = MutationReceipt {
            project_id: receipt.asset.get("project_id").and_then(Value::as_str).unwrap_or_default().into(),
            asset_id: receipt.asset.get("asset_id").and_then(Value::as_str).unwrap_or_default().into(),
            version: receipt.asset.get("version").and_then(Value::as_u64).unwrap_or(0),
            audit_id: receipt.audit_id.clone(),
            changed: receipt.changed.clone(),
        };
        let rooms = ROOMS.with(Rc::clone);
        timeline::classified(&edge, &rooms, scope.clone(), scope, asset_id, expected_version, mutation).await;
        typed(&receipt)
    }

    /// Attach a link to one row, or detach it.
    #[wasm_bindgen(js_name = assetsLink, unchecked_return_type = "Record<string, unknown>")]
    pub async fn assets_link(
        host: GatewayHost,
        project: String,
        asset_id: String,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] link: JsValue,
        detach: bool,
    ) -> Result<JsValue, JsValue> {
        let link: Value = input(link)?;
        typed(&ask(&host, Relink { project, asset_id, link, detach }).await?)
    }

    type Settled = Result<TimelinePage, Refusal>;

    /// Where a read that joined a page in flight receives its answer.
    #[derive(Default)]
    struct Slot {
        value: Option<Settled>,
        waker: Option<std::task::Waker>,
    }

    thread_local! {
        /// This tab's timeline coordinator: epochs and reads in flight.
        static ROOMS: Rc<RefCell<Rooms>> = Rc::default();
        /// Reads that joined a page already in flight, by page key.
        static JOINED: RefCell<BTreeMap<String, Vec<Rc<RefCell<Slot>>>>> = RefCell::default();
    }

    async fn joined(key: String) -> Settled {
        let slot = Rc::new(RefCell::new(Slot::default()));
        JOINED.with(|joined| joined.borrow_mut().entry(key).or_default().push(Rc::clone(&slot)));
        std::future::poll_fn(|context| {
            let mut slot = slot.borrow_mut();
            match slot.value.take() {
                Some(value) => std::task::Poll::Ready(value),
                None => {
                    slot.waker = Some(context.waker().clone());
                    std::task::Poll::Pending
                }
            }
        })
        .await
    }

    fn settle_joined(key: &str, page: &Settled) {
        for slot in JOINED.with(|joined| joined.borrow_mut().remove(key)).unwrap_or_default() {
            let mut slot = slot.borrow_mut();
            slot.value = Some(page.clone());
            if let Some(waker) = slot.waker.take() {
                waker.wake();
            }
        }
    }

    /// One page of the project's index for the timeline: a fresh held page
    /// at once, offline what this device holds, otherwise the network's page,
    /// retained for the next open. `onSaved` receives this device's older copy
    /// at once while the network is asked.
    #[wasm_bindgen(js_name = assetsTimelinePage, unchecked_return_type = "AssetsIndexAnswer")]
    pub async fn assets_timeline_page(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "AssetsScope")] scope: JsValue,
        #[wasm_bindgen(unchecked_param_type = "IndexQuery | undefined")] query: JsValue,
        #[wasm_bindgen(unchecked_param_type = "((page: AssetsIndexAnswer) => void) | undefined")] on_saved: JsValue,
    ) -> Result<JsValue, JsValue> {
        let scope: Scope = input(scope)?;
        let query: IndexQuery = if query.is_undefined() || query.is_null() { IndexQuery::default() } else { input(query)? };
        let rooms = ROOMS.with(Rc::clone);
        let edge = BrowserEdge::new(host, store);
        let saved = |page: &TimelinePage| {
            if let (Some(callback), Ok(page)) = (on_saved.dyn_ref::<js_sys::Function>(), typed(page)) {
                let _ = callback.call1(&JsValue::NULL, &page);
            }
        };
        let page = match begin(&edge, &rooms, scope, query, saved).await {
            Ok(Begun::Answer(page)) => Ok(page),
            Ok(Begun::Join(ticket)) => joined(ticket.key).await,
            Ok(Begun::Acquire(acquisition)) => {
                let key = acquisition.ticket.key.clone();
                let page = acquire(&edge, &rooms, acquisition).await;
                settle_joined(&key, &page);
                page
            }
            Err(refusal) => Err(refusal),
        };
        typed(&page.map_err(|refusal| rejection(&refusal))?)
    }

    /// One index row with its versions, newest first.
    #[wasm_bindgen(js_name = assetsIndexRow, unchecked_return_type = "RowAnswer")]
    pub async fn assets_index_row(host: GatewayHost, project: String, asset_id: String) -> Result<JsValue, JsValue> {
        match attempt(&host, IndexGet { project, asset_id }).await? {
            Ok(answer) => typed(&answer),
            Err(refusal) if predates_index(&refusal) => typed(&RowAnswer::Unavailable),
            Err(refusal) => Err(rejection(&refusal)),
        }
    }
}

#[cfg(test)]
#[path = "assets_tests.rs"]
mod tests;
