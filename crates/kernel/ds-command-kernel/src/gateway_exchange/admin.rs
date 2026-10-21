//! Rwanda's administrative units: the children of one unit for cascading
//! choices, one unit's boundary, every unit of a level under one parent with
//! its boundary, one unit's name, and the units containing a point. Brain
//! reads the boundary tables; these reads name the query and admit the rows.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{data, Admitted, Exchange, Method, Plan, Refusal, Request, DEFAULT_TIMEOUT_MS};

const PATH: &str = "/api/v1/admin/rwanda";
/// Every village of a district with its boundary.
const BOUNDARIES_CAP: u64 = 64 * 1024 * 1024;
const MAX_TIMEOUT_MS: u64 = 600_000;
const CODE_MAX_CHARS: usize = 16;
pub const INVALID: &str = "admin_answer_invalid";
pub const INPUT_INVALID: &str = "admin_input_invalid";

/// The administrative levels, largest first.
pub const LEVELS: [&str; 5] = ["province", "district", "sector", "cell", "village"];

/// One unit as a cascading choice.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct AdminChild {
    pub code: String,
    pub name: String,
}

/// One unit with its boundary; villages carry their cell, sector and district.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct AdminBoundary {
    pub level: String,
    pub code: String,
    pub name: String,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "GeoJSON.Geometry | null")
    )]
    pub geometry: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub cell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub district: Option<String>,
}

/// One unit's name.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct AdminName {
    pub level: String,
    pub code: String,
    pub name: String,
}

/// The unit of one level containing a point, with its district and province.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct AdminLocation {
    pub found: bool,
    pub level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub district: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub district_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub province: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub province_code: Option<String>,
}

fn invalid() -> Refusal {
    Refusal::new(
        INVALID,
        "The administrative-boundary service answered an invalid result.",
    )
}

fn input_invalid(message: &str) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

fn level(level: &str) -> Result<&str, Refusal> {
    LEVELS
        .contains(&level)
        .then_some(level)
        .ok_or_else(|| input_invalid("A level is province, district, sector, cell or village."))
}

fn code(code: &str) -> Result<&str, Refusal> {
    let valid = !code.is_empty()
        && code.chars().count() <= CODE_MAX_CHARS
        && code.chars().all(|c| c.is_ascii_digit());
    valid
        .then_some(code)
        .ok_or_else(|| input_invalid("An administrative code is up to 16 digits."))
}

fn deadline(timeout_ms: Option<u64>) -> Result<u64, Refusal> {
    match timeout_ms {
        None => Ok(DEFAULT_TIMEOUT_MS),
        Some(ms) if (1_000..=MAX_TIMEOUT_MS).contains(&ms) => Ok(ms),
        Some(_) => Err(input_invalid("A boundary read waits 1 to 600 seconds.")),
    }
}

fn read(query: Vec<(&str, String)>, timeout_ms: u64) -> Request {
    Request {
        method: Method::Get,
        body: None,
        query: query
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
        ..Request::post(PATH, Value::Null)
    }
    .timeout_ms(timeout_ms)
}

fn boundary_from(value: &Value, fallback_level: &str) -> Result<AdminBoundary, Refusal> {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
    let code = text("code")
        .filter(|code| !code.is_empty())
        .ok_or_else(invalid)?;
    let geometry = value
        .get("geometry")
        .filter(|geometry| geometry.is_object())
        .cloned();
    Ok(AdminBoundary {
        level: text("level")
            .filter(|level| !level.is_empty())
            .unwrap_or_else(|| fallback_level.into()),
        code,
        name: text("name").unwrap_or_default(),
        geometry,
        cell: text("cell"),
        sector: text("sector"),
        district: text("district"),
    })
}

/// The units of `level`, all of them or those under `parent_code`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Children {
    pub level: String,
    pub parent_code: Option<String>,
    pub timeout_ms: Option<u64>,
}

impl Exchange for Children {
    type Output = Vec<AdminChild>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut query = vec![
            ("action", "children".into()),
            ("level", level(&self.level)?.into()),
        ];
        if let Some(parent) = self
            .parent_code
            .as_deref()
            .filter(|parent| !parent.is_empty())
        {
            query.push(("parent_code", code(parent)?.into()));
        }
        Ok(read(query, deadline(self.timeout_ms)?).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let rows = data(&body)
            .get("children")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        rows.iter()
            .map(|row| AdminChild::deserialize(row).map_err(|_| invalid()))
            .collect::<Result<_, _>>()
            .map(Admitted::Done)
    }
}

/// One unit's boundary as GeoJSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Boundary {
    pub code: String,
    pub timeout_ms: Option<u64>,
}

impl Exchange for Boundary {
    type Output = AdminBoundary;

    fn plan(&self) -> Result<Plan, Refusal> {
        let query = vec![
            ("action", "geometry".into()),
            ("code", code(&self.code)?.into()),
        ];
        Ok(read(query, deadline(self.timeout_ms)?).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        boundary_from(data(&body), "").map(Admitted::Done)
    }
}

/// Every unit of `level` under one parent, each with its boundary, in one read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descendants {
    pub level: String,
    pub parent_code: String,
    pub timeout_ms: Option<u64>,
}

impl Exchange for Descendants {
    type Output = Vec<AdminBoundary>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let query = vec![
            ("action", "descendants".into()),
            ("level", level(&self.level)?.into()),
            ("parent_code", code(&self.parent_code)?.into()),
        ];
        Ok(read(query, deadline(self.timeout_ms)?)
            .response_cap(BOUNDARIES_CAP)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let rows = data(&body)
            .get("boundaries")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?;
        rows.iter()
            .map(|row| boundary_from(row, &self.level))
            .collect::<Result<_, _>>()
            .map(Admitted::Done)
    }
}

/// One unit's name, without its boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub code: String,
}

impl Exchange for Name {
    type Output = AdminName;

    fn plan(&self) -> Result<Plan, Refusal> {
        let query = vec![
            ("action", "query".into()),
            ("code", code(&self.code)?.into()),
        ];
        Ok(read(query, DEFAULT_TIMEOUT_MS).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let text = |key: &str| answer.get(key).and_then(Value::as_str).map(str::to_owned);
        Ok(Admitted::Done(AdminName {
            level: text("level").unwrap_or_default(),
            code: text("code").unwrap_or(self.code),
            name: text("name").ok_or_else(invalid)?,
        }))
    }
}

/// The unit of `level` containing a point; `village` is the deepest.
#[derive(Clone, Debug, PartialEq)]
pub struct Locate {
    pub lat: f64,
    pub lon: f64,
    pub level: String,
}

impl Exchange for Locate {
    type Output = AdminLocation;

    fn plan(&self) -> Result<Plan, Refusal> {
        if !(-90.0..=90.0).contains(&self.lat) || !(-180.0..=180.0).contains(&self.lon) {
            return Err(input_invalid(
                "A point has a latitude and a longitude in degrees.",
            ));
        }
        let query = vec![
            ("action", "locate".into()),
            ("lat", self.lat.to_string()),
            ("lon", self.lon.to_string()),
            ("level", level(&self.level)?.into()),
        ];
        Ok(read(query, DEFAULT_TIMEOUT_MS).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let mut location = AdminLocation::deserialize(data(&body)).map_err(|_| invalid())?;
        if location.level.is_empty() {
            location.level = self.level;
        }
        Ok(Admitted::Done(location))
    }
}

/// The key a level's children are held under: `level:parent_code`.
#[cfg(feature = "durable-store")]
fn children_key(level: &str, parent_code: Option<&str>) -> String {
    format!("{level}:{}", parent_code.unwrap_or_default())
}

/// The children of one unit this device holds, with no transport.
#[cfg(feature = "durable-store")]
pub async fn held_children(
    edge: &impl super::retained::Edge,
    level: &str,
    parent_code: Option<&str>,
) -> Result<Option<Vec<AdminChild>>, Refusal> {
    let key = children_key(level, parent_code);
    let kind = crate::retained_reads::Kind::AdminChildren;
    let observed = super::retained::read(edge, kind, None, &key).await?;
    Ok(observed.held.map(|held| held.value))
}

/// The children of one unit from Brain, held under the account its read
/// observed so cascading choices open offline. Holding is a cache: a store
/// that cannot keep them never hides them.
#[cfg(feature = "durable-store")]
pub async fn refresh_children(
    edge: &impl super::retained::Edge,
    call: Children,
) -> Result<Vec<AdminChild>, Refusal> {
    let key = children_key(&call.level, call.parent_code.as_deref());
    let kind = crate::retained_reads::Kind::AdminChildren;
    let observed = super::retained::read::<Vec<AdminChild>>(edge, kind, None, &key)
        .await
        .ok();
    let rows = edge.fetch(call).await?;
    if let Some(observed) = observed {
        let owner = observed.owner.as_deref();
        let _ = super::retained::write(edge, owner, kind, None, &key, &rows, None).await;
    }
    Ok(rows)
}

/// One unit's boundary: what this device holds while offline or when no
/// answer arrives, else Brain's, kept for the next open.
#[cfg(feature = "durable-store")]
pub async fn boundary(
    edge: &impl super::retained::Edge,
    call: Boundary,
) -> Result<AdminBoundary, Refusal> {
    let kind = crate::retained_reads::Kind::AdminBoundary;
    let key = call.code.clone();
    super::retained::held_or_fresh(edge, kind, None, &key, call).await
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{boundary, held_children, refresh_children, Boundary, Children, Descendants, Locate, Name};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, run, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use wasm_bindgen::prelude::*;

    fn deadline(timeout_ms: Option<u32>) -> Option<u64> {
        timeout_ms.map(u64::from)
    }

    /// The units of `level`, all of them or those under `parentCode`.
    #[wasm_bindgen(js_name = adminChildren, unchecked_return_type = "AdminChild[]")]
    pub async fn admin_children(
        host: GatewayHost,
        level: String,
        parent_code: Option<String>,
        timeout_ms: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        let call = Children {
            level,
            parent_code,
            timeout_ms: deadline(timeout_ms),
        };
        typed(&run(&host, call).await?)
    }

    /// The children of one unit this device holds, at once and offline.
    #[wasm_bindgen(js_name = adminChildrenHeld, unchecked_return_type = "AdminChild[] | undefined")]
    pub async fn admin_children_held(
        host: GatewayHost,
        store: StoreHost,
        level: String,
        parent_code: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let held = held_children(&edge, &level, parent_code.as_deref()).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// The children of one unit from Brain, held for the next offline open.
    #[wasm_bindgen(js_name = adminChildrenRefresh, unchecked_return_type = "AdminChild[]")]
    pub async fn admin_children_refresh(
        host: GatewayHost,
        store: StoreHost,
        level: String,
        parent_code: Option<String>,
        timeout_ms: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let call = Children {
            level,
            parent_code,
            timeout_ms: deadline(timeout_ms),
        };
        typed(
            &refresh_children(&edge, call)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    /// One unit's boundary as GeoJSON: held while offline, else fresh and
    /// kept.
    #[wasm_bindgen(js_name = adminBoundary, unchecked_return_type = "AdminBoundary")]
    pub async fn admin_boundary(
        host: GatewayHost,
        store: StoreHost,
        code: String,
        timeout_ms: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let call = Boundary {
            code,
            timeout_ms: deadline(timeout_ms),
        };
        typed(
            &boundary(&edge, call)
                .await
                .map_err(|refusal| rejection(&refusal))?,
        )
    }

    /// Every unit of `level` under one parent, each with its boundary.
    #[wasm_bindgen(js_name = adminDescendants, unchecked_return_type = "AdminBoundary[]")]
    pub async fn admin_descendants(
        host: GatewayHost,
        level: String,
        parent_code: String,
        timeout_ms: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        let call = Descendants {
            level,
            parent_code,
            timeout_ms: deadline(timeout_ms),
        };
        typed(&run(&host, call).await?)
    }

    /// One unit's name.
    #[wasm_bindgen(js_name = adminName, unchecked_return_type = "AdminName")]
    pub async fn admin_name(host: GatewayHost, code: String) -> Result<JsValue, JsValue> {
        typed(&run(&host, Name { code }).await?)
    }

    /// The unit of `level` containing a point.
    #[wasm_bindgen(js_name = adminLocate, unchecked_return_type = "AdminLocation")]
    pub async fn admin_locate(
        host: GatewayHost,
        lat: f64,
        lon: f64,
        level: String,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, Locate { lat, lon, level }).await?)
    }
}

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;
