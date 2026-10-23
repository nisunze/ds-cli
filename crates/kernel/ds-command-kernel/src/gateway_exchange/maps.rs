//! Map provider configuration: a project's Google dark roadmap session.
//! Brain holds the provider key and answers ready-to-use tile URLs; this
//! operation names the request and admits the answer. A project with no dark
//! tiles to offer reads as `None`.

use ds_gateway_client::Operation;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

/// The answer is a few hundred bytes of configuration.
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
        Ok(Request::generated(Operation::Maps2DAction, Some(body))?
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
