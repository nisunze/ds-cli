//! Platform routing for vector-tile compute.
//!
//! Source acquisition and publication remain authenticated service effects.
//! This owner decides only where Tippecanoe/PMTiles compute runs. Keeping that
//! decision here prevents browser code, a Desktop shell and the Linux Server
//! from growing different interpretations of the same platform identity.

use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "ds.tiling-placement/v1";
pub const MAX_INPUT_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Web,
    Desktop,
    Server,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Linux,
    Windows,
    Macos,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub operation: String,
    pub surface: Surface,
    #[serde(default)]
    pub platform: Option<Platform>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputeRoute {
    LocalNative,
    OnlineContainer,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Placement {
    pub schema: &'static str,
    pub operation: &'static str,
    pub compute_route: ComputeRoute,
    pub source_route: &'static str,
    pub publication_route: &'static str,
    pub cache: &'static str,
}

pub fn resolve(request: Request) -> Result<Placement, String> {
    if request.schema != SCHEMA {
        return Err("Unsupported tiling placement schema.".into());
    }
    if request.operation != "tiles.build" {
        return Err("Unsupported tiling operation.".into());
    }

    let compute_route = match (request.surface, request.platform) {
        (Surface::Web, None) => ComputeRoute::OnlineContainer,
        (Surface::Web, Some(_)) => {
            return Err("A web tiling request must not claim an operating system.".into());
        }
        (Surface::Desktop, Some(Platform::Linux)) | (Surface::Server, Some(Platform::Linux)) => {
            ComputeRoute::LocalNative
        }
        (Surface::Desktop, Some(Platform::Windows | Platform::Macos)) => {
            ComputeRoute::OnlineContainer
        }
        (Surface::Desktop | Surface::Server, None) => {
            return Err("Installed tiling requires an explicit platform.".into());
        }
        (Surface::Server, Some(Platform::Windows | Platform::Macos)) => {
            return Err("The native tiling Server is supported on Linux only.".into());
        }
    };

    Ok(Placement {
        schema: SCHEMA,
        operation: "tiles.build",
        compute_route,
        source_route: "online_service",
        publication_route: "online_service",
        cache: match compute_route {
            ComputeRoute::LocalNative => "local_content_addressed",
            ComputeRoute::OnlineContainer => "service_managed",
        },
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Tiling placement command exceeds its bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid tiling placement command: {error}"))?;
    serde_json::to_string(&resolve(request)?).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn route(surface: &str, platform: Option<&str>) -> Result<Value, String> {
        let mut request = json!({
            "schema": SCHEMA,
            "operation": "tiles.build",
            "surface": surface,
        });
        if let Some(platform) = platform {
            request["platform"] = json!(platform);
        }
        evaluate(&serde_json::to_vec(&request).unwrap())
            .map(|answer| serde_json::from_str(&answer).expect("placement answer is JSON"))
    }

    #[test]
    fn platform_routes_are_explicit_and_keep_online_effects_outside_compute() {
        for (surface, platform, expected, cache) in [
            ("web", None, "online_container", "service_managed"),
            (
                "desktop",
                Some("windows"),
                "online_container",
                "service_managed",
            ),
            (
                "desktop",
                Some("macos"),
                "online_container",
                "service_managed",
            ),
            (
                "desktop",
                Some("linux"),
                "local_native",
                "local_content_addressed",
            ),
            (
                "server",
                Some("linux"),
                "local_native",
                "local_content_addressed",
            ),
        ] {
            let answer = route(surface, platform).unwrap();
            assert_eq!(answer["compute_route"], expected);
            assert_eq!(answer["cache"], cache);
            assert_eq!(answer["source_route"], "online_service");
            assert_eq!(answer["publication_route"], "online_service");
        }
    }

    #[test]
    fn malformed_or_impossible_platform_claims_fail_closed() {
        assert!(route("web", Some("linux")).is_err());
        assert!(route("desktop", None).is_err());
        assert!(route("server", Some("windows")).is_err());
        assert!(evaluate(
            br#"{"schema":"ds.tiling-placement/v1","operation":"tiles.other","surface":"web"}"#
        )
        .is_err());
        assert!(evaluate(br#"{"schema":"ds.tiling-placement/v1","operation":"tiles.build","surface":"web","force":true}"#).is_err());
    }
}
