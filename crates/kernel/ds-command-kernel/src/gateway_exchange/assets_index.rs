//! The Assets timeline: one project's index, a page at a time, as ds-brain
//! builds and serves it (ds-brain/docs/contracts/assets-index.md §5.1).
//!
//! These exchanges name the index request and admit its answer: a row
//! without its identity is dropped and counted, never half-rendered. A
//! ds-brain that predates the index refuses `view` with 400
//! `VALIDATION_FAILED`; that reads as `unavailable`, never as an empty index.
//! The timeline flow that retains each served page is [`timeline`].

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

#[cfg(feature = "durable-store")]
pub mod timeline;

const PATH: &str = "/api/v1/assets";
/// A cold project index can enumerate many source inventories before its
/// first stored generation exists; later reads serve that generation.
const TIMEOUT_MS: u64 = 300_000;
/// A page holds at most 200 rows.
const RESPONSE_CAP: u64 = 8 * 1024 * 1024;
pub const INVALID: &str = "assets_index_answer_invalid";
pub const INTEGRITY: &str = "ASSET_INTEGRITY";

/// What the timeline asks for. `refresh` asks ds-brain to rebuild the index.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct IndexQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub order: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub folder: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub refresh: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct IndexVersions {
    pub count: u64,
    pub current: Option<String>,
    pub latest_at: Option<String>,
}

/// One row of the index: the asset row as ds-brain serves it, its identity,
/// folder path, modification time and version summary admitted.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct IndexRow {
    pub asset_id: String,
    pub name: String,
    pub folder_path: String,
    pub modified_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub versions: Option<IndexVersions>,
    /// Every other member of the row, exactly as served.
    #[serde(flatten)]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "Record<string, unknown>")
    )]
    pub row: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct IndexTruncation {
    pub source: String,
    pub bound: f64,
    pub actual: f64,
}

/// The index generation a page was served from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct IndexMeta {
    pub schema: String,
    pub generation: f64,
    pub built_at: Option<String>,
    pub checked_at: Option<String>,
    pub rebuilt: bool,
    pub stale_sources: Vec<String>,
    pub truncated_sources: Vec<IndexTruncation>,
}

/// One served page.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IndexPage {
    pub assets: Vec<IndexRow>,
    pub more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub index: Option<IndexMeta>,
    pub dropped: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct IndexVersionRow {
    pub asset_id: String,
    pub label: String,
    pub created_at: Option<String>,
    pub created_by: Option<String>,
    pub reason: Option<String>,
    pub milestone: Option<String>,
    pub current: bool,
}

/// One row and its versions, newest first.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RowAnswer {
    Served {
        asset: IndexRow,
        versions: Vec<IndexVersionRow>,
    },
    Unavailable,
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}
fn number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite())
}
fn objects(value: Option<&Value>) -> impl Iterator<Item = &Map<String, Value>> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
}

/// A row with its identity, or `None`.
pub fn decode_row(value: &Value) -> Option<IndexRow> {
    let raw = value.as_object()?;
    let asset_id = text(raw.get("asset_id"))?;
    let name = text(raw.get("name"))?;
    let versions = raw
        .get("versions")
        .and_then(Value::as_object)
        .and_then(|versions| {
            let count = number(versions.get("count")).filter(|count| *count >= 1.0)?;
            Some(IndexVersions {
                count: count as u64,
                current: text(versions.get("current")),
                latest_at: text(versions.get("latest_at")),
            })
        });
    let folder_path = text(raw.get("folder_path"))
        .or_else(|| text(raw.get("folder")))
        .unwrap_or_default();
    let mut row = raw.clone();
    for admitted in ["asset_id", "name", "folder_path", "modified_at", "versions"] {
        row.remove(admitted);
    }
    Some(IndexRow {
        asset_id,
        name,
        folder_path,
        modified_at: text(raw.get("modified_at")),
        versions,
        row,
    })
}

fn decode_meta(value: Option<&Value>) -> Option<IndexMeta> {
    let raw = value?.as_object()?;
    Some(IndexMeta {
        schema: text(raw.get("schema")).unwrap_or_default(),
        generation: number(raw.get("generation")).unwrap_or(0.0),
        built_at: text(raw.get("built_at")),
        checked_at: text(raw.get("checked_at")),
        rebuilt: raw.get("rebuilt") == Some(&Value::Bool(true)),
        stale_sources: raw
            .get("stale_sources")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|name| text(Some(name)))
            .collect(),
        truncated_sources: objects(raw.get("truncated_sources"))
            .map(|entry| IndexTruncation {
                source: text(entry.get("source")).unwrap_or_default(),
                bound: number(entry.get("bound")).unwrap_or(0.0),
                actual: number(entry.get("actual")).unwrap_or(0.0),
            })
            .collect(),
    })
}

fn body(action: &str, project: &str, payload: Map<String, Value>) -> Result<Value, Refusal> {
    let mut body =
        json!({"action": action, "ds_project": project_reference(project)?, "view": "index"});
    body.as_object_mut().expect("an object").extend(payload);
    Ok(body)
}

/// One page of the index, newest first unless asked otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexList {
    pub project: String,
    pub query: IndexQuery,
}

impl Exchange for IndexList {
    type Output = IndexPage;

    fn plan(&self) -> Result<Plan, Refusal> {
        let query = &self.query;
        let mut payload = Map::new();
        payload.insert(
            "order".into(),
            query
                .order
                .clone()
                .unwrap_or_else(|| "recent".into())
                .into(),
        );
        for (name, value) in [
            ("folder", &query.folder),
            ("kind", &query.kind),
            ("status", &query.status),
            ("sensitivity", &query.sensitivity),
            ("since", &query.since),
            ("cursor", &query.cursor),
        ] {
            if let Some(value) = value.as_deref().filter(|value| !value.is_empty()) {
                payload.insert(name.into(), value.into());
            }
        }
        if let Some(limit) = query.limit.filter(|limit| *limit > 0) {
            payload.insert("limit".into(), limit.into());
        }
        if query.refresh == Some(true) {
            payload.insert("refresh".into(), true.into());
        }
        Ok(Request::post(PATH, body("list", &self.project, payload)?)
            .timeout_ms(TIMEOUT_MS)
            .response_cap(RESPONSE_CAP)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let raw = answer
            .get("assets")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let assets: Vec<IndexRow> = raw.iter().filter_map(decode_row).collect();
        Ok(Admitted::Done(IndexPage {
            dropped: (raw.len() - assets.len()) as u64,
            assets,
            more: answer.get("more") == Some(&Value::Bool(true)),
            next_cursor: text(answer.get("next_cursor")),
            index: decode_meta(answer.get("index")),
        }))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// One row of the index and its versions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexGet {
    pub project: String,
    pub asset_id: String,
}

impl Exchange for IndexGet {
    type Output = RowAnswer;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut payload = Map::new();
        payload.insert("asset_id".into(), self.asset_id.clone().into());
        Ok(Request::post(PATH, body("get", &self.project, payload)?)
            .timeout_ms(TIMEOUT_MS)
            .response_cap(RESPONSE_CAP)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let asset = answer
            .get("asset")
            .and_then(decode_row)
            .ok_or_else(|| Refusal::new(INTEGRITY, "The index answered without a readable row."))?;
        let versions = objects(answer.get("versions"))
            .filter_map(|raw| {
                let asset_id = text(raw.get("asset_id"))?;
                Some(IndexVersionRow {
                    label: text(raw.get("label")).unwrap_or_else(|| asset_id.clone()),
                    asset_id,
                    created_at: text(raw.get("created_at")),
                    created_by: text(raw.get("created_by")),
                    reason: text(raw.get("reason")),
                    milestone: text(raw.get("milestone")),
                    current: raw.get("current") == Some(&Value::Bool(true)),
                })
            })
            .collect();
        Ok(Admitted::Done(RowAnswer::Served { asset, versions }))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// Only a ds-brain that predates the index answers this to `view`.
pub fn predates_index(refusal: &Refusal) -> bool {
    refusal.status == Some(400) && refusal.code.eq_ignore_ascii_case("VALIDATION_FAILED")
}

#[cfg(test)]
#[path = "assets_index_tests.rs"]
mod tests;
