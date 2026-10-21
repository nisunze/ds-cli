//! The gateway a page build speaks to: constants of the build, compiled by
//! `build_gateway.rs` from the lane feature and the project's build-time
//! environment. A host never names an origin, a lane or a key; the build
//! identity records what it was compiled for, the key only as its digest.

use serde::Serialize;

#[cfg(all(feature = "lane-canary", feature = "lane-production"))]
compile_error!("a Kernel build serves one lane: lane-canary or lane-production, not both");
#[cfg(all(
    feature = "topology-local",
    not(any(feature = "lane-canary", feature = "lane-production"))
))]
compile_error!("topology-local names the lane its local stack stands in for");
#[cfg(all(
    feature = "wasm",
    target_arch = "wasm32",
    not(any(feature = "lane-canary", feature = "lane-production"))
))]
compile_error!(
    "a page Kernel build names its target: build it through the `targets` of release/wasm-unit.json"
);

include!(concat!(env!("OUT_DIR"), "/gateway_target.rs"));

/// The release authorities a page may read across lanes.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct ReleaseOrigins {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canary: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production: Option<&'static str>,
}

/// What this build was compiled to speak to.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct GatewayTarget {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "'canary' | 'production'")
    )]
    pub lane: Option<&'static str>,
    /// The local stack: the page's own origin proxies the gateway.
    pub local: bool,
    /// The deployed gateway; absent on the local stack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway_origin: Option<&'static str>,
    pub release_origins: ReleaseOrigins,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firebase_project: Option<&'static str>,
}

/// This build's gateway target.
pub fn target() -> GatewayTarget {
    GatewayTarget {
        lane: LANE,
        local: LOCAL,
        gateway_origin: GATEWAY_ORIGIN,
        release_origins: ReleaseOrigins {
            canary: RELEASE_ORIGIN_CANARY,
            production: RELEASE_ORIGIN_PRODUCTION,
        },
        firebase_project: FIREBASE_PROJECT,
    }
}

/// What this build was compiled for, as JSON; the key appears only as its
/// SHA-256.
pub fn identity() -> &'static str {
    IDENTITY
}

/// The audience a store identity and a held row are scoped to: this build's
/// Firebase project and the gateway its requests reach (`gateway`, the page's
/// own origin on the local stack), in the shape the Kernel store already
/// binds, so no held row changes owner.
pub fn audience(gateway: &str) -> Option<String> {
    let project = FIREBASE_PROJECT?;
    Some(serde_json::json!({"firebaseProjectId": project, "gateway": gateway}).to_string())
}

/// One release channel's deployed gateway, as this build was compiled: what
/// a native client (Core) needs to speak to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseGateway {
    pub origin: &'static str,
    pub api_key: &'static str,
    pub firebase_project: &'static str,
    pub firebase_api_key: &'static str,
}

/// The gateway of release `channel` (a `releaseTargets` key: `stable`,
/// `canary`), or the build variable this build was compiled without.
pub fn release_gateway(channel: &str) -> Result<ReleaseGateway, &'static str> {
    let (_, lane) = RELEASE_CHANNELS
        .iter()
        .find(|(name, _)| *name == channel)
        .ok_or("releaseTargets")?;
    let origin = if *lane == "canary" {
        RELEASE_ORIGIN_CANARY.ok_or("DS_GATEWAY_URL_CANARY")?
    } else {
        RELEASE_ORIGIN_PRODUCTION.ok_or("DS_GATEWAY_URL_PRODUCTION")?
    };
    Ok(ReleaseGateway {
        origin,
        api_key: PLATFORM_API_KEY.ok_or("DS_PLATFORM_API_KEY")?,
        firebase_project: FIREBASE_PROJECT.ok_or("DS_FIREBASE_PROJECT_ID")?,
        firebase_api_key: FIREBASE_API_KEY.ok_or("DS_FIREBASE_API_KEY")?,
    })
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use wasm_bindgen::prelude::*;

    use crate::gateway_exchange::browser::typed;

    /// The gateway this build speaks to.
    #[wasm_bindgen(js_name = gatewayTarget, unchecked_return_type = "GatewayTarget")]
    pub fn gateway_target() -> Result<JsValue, JsValue> {
        typed(&super::target())
    }

    /// The audience held rows and the store identity are scoped to.
    #[wasm_bindgen(js_name = gatewayAudience)]
    pub fn gateway_audience() -> Result<String, JsValue> {
        let gateway = crate::gateway_exchange::browser::gateway_origin()?;
        super::audience(&gateway)
            .ok_or_else(|| js_sys::Error::new("this Kernel build names no Firebase project").into())
    }
}

#[cfg(test)]
#[path = "gateway_target_tests.rs"]
mod tests;
