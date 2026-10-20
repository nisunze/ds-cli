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

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use wasm_bindgen::prelude::*;

    use crate::gateway_exchange::browser::typed;

    /// The gateway this build speaks to.
    #[wasm_bindgen(js_name = gatewayTarget, unchecked_return_type = "GatewayTarget")]
    pub fn gateway_target() -> Result<JsValue, JsValue> {
        typed(&super::target())
    }
}

#[cfg(test)]
#[path = "gateway_target_tests.rs"]
mod tests;
