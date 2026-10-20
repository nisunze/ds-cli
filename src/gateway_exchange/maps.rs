//! Map provider configuration: a project's Google dark roadmap session and
//! the global 3D provider configuration. Brain holds the provider keys and
//! answers ready-to-use tile URLs; these operations name the request and
//! admit the answer. A project with no dark tiles to offer reads as `None`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

const MAPS2D_PATH: &str = "/api/v1/maps2d";
const MAPS3D_PATH: &str = "/api/v1/maps3d";
/// Both answers are a few hundred bytes of configuration.
const RESPONSE_CAP: u64 = 256 * 1024;
pub const INVALID: &str = "maps_answer_invalid";

/// One raster basemap, as the map's basemap catalogue carries it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Basemap {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub tiles: Vec<String>,
    #[serde(rename = "tileSize", default, skip_serializing_if = "Option::is_none")]
    pub tile_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct DarkSession {
    pub basemap: Basemap,
    #[serde(default)]
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Google3d {
    pub available: bool,
    /// The 3D Tiles root with the key embedded; empty when unavailable.
    pub root_url: String,
    pub attribution: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct AwsTerrain {
    pub available: bool,
    pub tiles_url: String,
    pub encoding: String,
    pub max_zoom: u32,
    pub default_exaggeration: f64,
    pub attribution: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Config3d {
    pub default_provider: String,
    pub google: Google3d,
    pub aws_terrain: AwsTerrain,
}

fn invalid(what: &str) -> Refusal {
    Refusal::new(
        INVALID,
        format!("The map service answered an invalid {what}."),
    )
}

/// The project's Google dark roadmap tile session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoogleDarkSession {
    pub project: String,
}

impl Exchange for GoogleDarkSession {
    type Output = Option<DarkSession>;

    fn plan(&self) -> Result<Plan, Refusal> {
        let body = json!({
            "action": "get_google_dark_session",
            "ds_project": project_reference(&self.project)?,
        });
        Ok(Request::post(MAPS2D_PATH, body)
            .response_cap(RESPONSE_CAP)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let offers_tiles = answer
            .pointer("/basemap/tiles")
            .and_then(Value::as_array)
            .is_some_and(|tiles| !tiles.is_empty());
        if !offers_tiles {
            return Ok(Admitted::Done(None));
        }
        let session = DarkSession::deserialize(answer).map_err(|_| invalid("dark map session"))?;
        if session
            .basemap
            .tiles
            .iter()
            .any(|tile| tile.trim().is_empty())
        {
            return Err(invalid("dark map session"));
        }
        Ok(Admitted::Done(Some(session)))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// The global 3D provider configuration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Maps3dConfig;

impl Exchange for Maps3dConfig {
    type Output = Config3d;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::post(MAPS3D_PATH, json!({"action": "get_config"}))
            .response_cap(RESPONSE_CAP)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Config3d::deserialize(data(&body))
            .map(Admitted::Done)
            .map_err(|_| invalid("3D configuration"))
    }
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{GoogleDarkSession, Maps3dConfig};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// The project's Google dark roadmap tile session; `undefined` when the
    /// project has no dark tiles to offer.
    #[wasm_bindgen(
        js_name = mapsGoogleDarkSession,
        unchecked_return_type = "DarkSession | undefined"
    )]
    pub async fn google_dark_session(
        host: GatewayHost,
        project: String,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, GoogleDarkSession { project }).await?)
    }

    /// The global 3D provider configuration.
    #[wasm_bindgen(js_name = maps3dConfig, unchecked_return_type = "Config3d")]
    pub async fn maps3d_config(host: GatewayHost) -> Result<JsValue, JsValue> {
        typed(&run(&host, Maps3dConfig).await?)
    }
}
