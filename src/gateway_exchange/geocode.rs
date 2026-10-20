//! Place search and reverse geocoding for the map's Search place dialogs.
//! Brain proxies Google with a key it holds; these calls name the query and
//! admit what comes back. Global: they carry no project. A deployment without
//! the key answers "unavailable", which is a state to show, never an error.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, Admitted, Exchange, Plan, Refusal, Request};

const PATH: &str = "/api/v1/geocode";
/// Brain's bound on a query, in bytes.
const QUERY_MAX_BYTES: usize = 256;
const LANGUAGE_MAX_CHARS: usize = 35;
/// The backend's code when this deployment holds no key.
pub const NOT_CONFIGURED: &str = "GEOCODE_NOT_CONFIGURED";
pub const INPUT_INVALID: &str = "geocode_input_invalid";
pub const INVALID: &str = "geocode_answer_invalid";

/// A point in degrees.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GeocodePoint {
    pub lat: f64,
    pub lng: f64,
}

/// One place a search found.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GeocodePlace {
    pub name: String,
    pub formatted_address: String,
    pub lat: f64,
    pub lng: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub place_id: Option<String>,
}

/// What a place search answers.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GeocodePlaces {
    /// False when this deployment holds no key.
    pub available: bool,
    pub places: Vec<GeocodePlace>,
}

/// One part of an address, such as its street or its city.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GeocodeAddressPart {
    pub long_name: String,
    pub types: Vec<String>,
}

/// What reverse geocoding answers for a point.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GeocodeAddress {
    /// False when this deployment holds no key.
    pub available: bool,
    /// False when no address is known here.
    pub found: bool,
    pub formatted_address: String,
    pub components: Vec<GeocodeAddressPart>,
}

fn input_invalid(message: &str) -> Refusal {
    Refusal::new(INPUT_INVALID, message)
}

fn invalid() -> Refusal {
    Refusal::new(INVALID, "Place search answered an invalid result.")
}

/// The point with its longitude wrapped into [-180, 180), or a refusal when
/// it is no point.
fn point(point: GeocodePoint) -> Result<GeocodePoint, Refusal> {
    if !point.lat.is_finite() || !point.lng.is_finite() || point.lat.abs() > 90.0 {
        return Err(input_invalid(
            "A point has a latitude and a longitude in degrees.",
        ));
    }
    Ok(GeocodePoint {
        lat: point.lat,
        lng: (point.lng + 180.0).rem_euclid(360.0) - 180.0,
    })
}

fn body(action: &str, language: Option<&str>) -> Result<Map<String, Value>, Refusal> {
    let mut body = Map::new();
    body.insert("action".into(), action.into());
    if let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) {
        let valid = language.chars().count() <= LANGUAGE_MAX_CHARS
            && language.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !valid {
            return Err(input_invalid("A language is a tag such as en or fr."));
        }
        body.insert("language".into(), language.into());
    }
    Ok(body)
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// A list member of an answer: absent or null is empty, anything but a list
/// is an invalid answer.
fn list<'a>(answer: &'a Value, key: &str) -> Result<&'a [Value], Refusal> {
    match answer.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(rows)) => Ok(rows),
        Some(_) => Err(invalid()),
    }
}

/// Whether this deployment can search places.
#[derive(Clone, Debug, PartialEq)]
pub struct Availability;

impl Exchange for Availability {
    type Output = bool;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::post(PATH, json!({"action": "get_config"})).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let available = data(&body).get("available") == Some(&Value::Bool(true));
        Ok(Admitted::Done(available))
    }
}

/// Places matching free text, ranked near `bias` when it is given.
#[derive(Clone, Debug, PartialEq)]
pub struct Search {
    pub query: String,
    pub language: Option<String>,
    pub bias: Option<GeocodePoint>,
}

impl Exchange for Search {
    type Output = Vec<GeocodePlace>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let query = self.query.trim();
        if query.is_empty() || query.len() > QUERY_MAX_BYTES {
            return Err(input_invalid("A place search is between 1 and 256 bytes long."));
        }
        let mut body = body("search", self.language.as_deref())?;
        body.insert("query".into(), query.into());
        if let Some(bias) = self.bias {
            let bias = point(bias)?;
            body.insert("bias_lat".into(), bias.lat.into());
            body.insert("bias_lng".into(), bias.lng.into());
        }
        Ok(Request::post(PATH, Value::Object(body)).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let places = list(data(&body), "results")?
            .iter()
            .filter_map(|row| {
                let name = text(row, "name")?.trim().to_owned();
                let lat = row.get("lat").and_then(Value::as_f64)?;
                let lng = row.get("lng").and_then(Value::as_f64)?;
                (!name.is_empty()).then(|| GeocodePlace {
                    name,
                    formatted_address: text(row, "formatted_address").unwrap_or_default(),
                    lat,
                    lng,
                    place_id: text(row, "place_id").filter(|id| !id.is_empty()),
                })
            })
            .collect();
        Ok(Admitted::Done(places))
    }
}

/// The address at a point.
#[derive(Clone, Debug, PartialEq)]
pub struct Reverse {
    pub point: GeocodePoint,
    pub language: Option<String>,
}

impl Exchange for Reverse {
    type Output = GeocodeAddress;

    fn plan(&self) -> Result<Plan, Refusal> {
        let at = point(self.point)?;
        let mut body = body("reverse", self.language.as_deref())?;
        body.insert("lat".into(), at.lat.into());
        body.insert("lng".into(), at.lng.into());
        Ok(Request::post(PATH, Value::Object(body)).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let components = list(answer, "components")?
            .iter()
            .filter(|part| part.is_object())
            .map(|part| GeocodeAddressPart {
                long_name: text(part, "long_name").unwrap_or_default(),
                types: part
                    .get("types")
                    .and_then(Value::as_array)
                    .map(|types| {
                        types
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect();
        Ok(Admitted::Done(GeocodeAddress {
            available: true,
            found: answer.get("found") == Some(&Value::Bool(true)),
            formatted_address: text(answer, "formatted_address").unwrap_or_default(),
            components,
        }))
    }
}

/// A search's outcome: its places, or unavailable when this deployment holds
/// no key.
pub fn places(outcome: Result<Vec<GeocodePlace>, Refusal>) -> Result<GeocodePlaces, Refusal> {
    match outcome {
        Ok(places) => Ok(GeocodePlaces {
            available: true,
            places,
        }),
        Err(refusal) if refusal.code == NOT_CONFIGURED => Ok(GeocodePlaces {
            available: false,
            places: Vec::new(),
        }),
        Err(refusal) => Err(refusal),
    }
}

/// A reverse lookup's outcome: its address, or unavailable when this
/// deployment holds no key.
pub fn address(outcome: Result<GeocodeAddress, Refusal>) -> Result<GeocodeAddress, Refusal> {
    match outcome {
        Err(refusal) if refusal.code == NOT_CONFIGURED => Ok(GeocodeAddress::default()),
        outcome => outcome,
    }
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{address, places, Availability, GeocodePoint, Reverse, Search};
    use crate::gateway_exchange::browser::{attempt, rejection, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// Whether this deployment can search places; false when it cannot be
    /// asked.
    #[wasm_bindgen(js_name = geocodeAvailable)]
    pub async fn geocode_available(host: GatewayHost) -> Result<bool, JsValue> {
        Ok(attempt(&host, Availability).await?.unwrap_or(false))
    }

    /// Places matching `query`, ranked near `bias` when it is given.
    #[wasm_bindgen(js_name = geocodeSearch, unchecked_return_type = "GeocodePlaces")]
    pub async fn geocode_search(
        host: GatewayHost,
        query: String,
        language: Option<String>,
        #[wasm_bindgen(unchecked_param_type = "GeocodePoint | undefined")] bias: JsValue,
    ) -> Result<JsValue, JsValue> {
        let bias = if bias.is_undefined() || bias.is_null() {
            None
        } else {
            Some(
                tsify::serde_wasm_bindgen::from_value::<GeocodePoint>(bias)
                    .map_err(|error| js_sys::Error::new(&error.to_string()))?,
            )
        };
        let call = Search {
            query,
            language,
            bias,
        };
        let outcome = places(attempt(&host, call).await?);
        typed(&outcome.map_err(|refusal| rejection(&refusal))?)
    }

    /// The address at a point.
    #[wasm_bindgen(js_name = geocodeReverse, unchecked_return_type = "GeocodeAddress")]
    pub async fn geocode_reverse(
        host: GatewayHost,
        lat: f64,
        lng: f64,
        language: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let call = Reverse {
            point: GeocodePoint { lat, lng },
            language,
        };
        let outcome = address(attempt(&host, call).await?);
        typed(&outcome.map_err(|refusal| rejection(&refusal))?)
    }
}

#[cfg(test)]
#[path = "geocode_tests.rs"]
mod tests;
