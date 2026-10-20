//! The gateway a page build speaks to, decided when the Kernel is built.
//!
//! The lane is a cargo feature (`lane-canary` or `lane-production`); the
//! local development stack adds `topology-local`, where the page's own origin
//! proxies the gateway. The gateway origins, the platform API key and the
//! Firebase project come from the project's build environment, never from a
//! repository:
//!
//!   DS_GATEWAY_URL_CANARY, DS_GATEWAY_URL_PRODUCTION, DS_PLATFORM_API_KEY,
//!   DS_FIREBASE_PROJECT_ID
//!
//! A deployed lane refuses to build without its gateway origin and the key.

use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

const VARS: [&str; 4] = [
    "DS_GATEWAY_URL_CANARY",
    "DS_GATEWAY_URL_PRODUCTION",
    "DS_PLATFORM_API_KEY",
    "DS_FIREBASE_PROJECT_ID",
];

fn fail(message: impl std::fmt::Display) -> ! {
    panic!("gateway_target_invalid: {message}")
}

fn feature(name: &str) -> bool {
    env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
}

fn var(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// An `https` origin with no path, query or credentials.
fn origin(name: &str) -> Option<String> {
    let value = var(name)?;
    let rest = value
        .strip_prefix("https://")
        .unwrap_or_else(|| fail(format!("{name} must be an https origin")));
    let host = rest.trim_end_matches('/');
    if host.is_empty() || host.contains(['/', '?', '#', '@']) {
        fail(format!(
            "{name} must be an origin with no path, query or credentials"
        ));
    }
    Some(format!("https://{host}"))
}

fn optional(value: &Option<String>) -> String {
    value
        .as_deref()
        .map_or_else(|| "None".to_owned(), |text| format!("Some({text:?})"))
}

/// Write `gateway_target.rs` into OUT_DIR for `src/gateway_target.rs`.
pub fn emit() {
    for name in VARS {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let lane = match (feature("LANE_CANARY"), feature("LANE_PRODUCTION")) {
        (true, true) => fail("a Kernel build serves one lane: lane-canary or lane-production"),
        (true, false) => Some("canary"),
        (false, true) => Some("production"),
        (false, false) => None,
    };
    let local = feature("TOPOLOGY_LOCAL");
    let canary = origin("DS_GATEWAY_URL_CANARY");
    let production = origin("DS_GATEWAY_URL_PRODUCTION");
    let key = var("DS_PLATFORM_API_KEY");
    let firebase = var("DS_FIREBASE_PROJECT_ID");
    // The gateway this build calls: the page's own origin on the local stack,
    // else the lane's deployed gateway, which must be named.
    let gateway = match (lane, local) {
        (_, true) | (None, false) => None,
        (Some("canary"), false) => Some(
            canary
                .clone()
                .unwrap_or_else(|| fail("lane-canary needs DS_GATEWAY_URL_CANARY")),
        ),
        (Some(_), false) => Some(
            production
                .clone()
                .unwrap_or_else(|| fail("lane-production needs DS_GATEWAY_URL_PRODUCTION")),
        ),
    };
    if lane.is_some() && !local && key.is_none() {
        fail("a deployed lane needs DS_PLATFORM_API_KEY");
    }
    let key_sha256 = key
        .as_deref()
        .map(|key| format!("{:x}", Sha256::digest(key.as_bytes())));
    let identity = serde_json::json!({
        "lane": lane,
        "topology": if local { "local" } else { "deployed" },
        "gateway_origin": gateway,
        "release_origins": {"canary": canary, "production": production},
        "api_key_sha256": key_sha256,
        "firebase_project": firebase,
    });
    let table = format!(
        "pub const LANE: Option<&str> = {lane};\n\
         pub const LOCAL: bool = {local};\n\
         pub const GATEWAY_ORIGIN: Option<&str> = {gateway};\n\
         pub const RELEASE_ORIGIN_CANARY: Option<&str> = {canary};\n\
         pub const RELEASE_ORIGIN_PRODUCTION: Option<&str> = {production};\n\
         pub const PLATFORM_API_KEY: Option<&str> = {key};\n\
         pub const FIREBASE_PROJECT: Option<&str> = {firebase};\n\
         pub const IDENTITY: &str = {identity:?};\n",
        lane = optional(&lane.map(str::to_owned)),
        gateway = optional(&gateway),
        canary = optional(&canary),
        production = optional(&production),
        key = optional(&key),
        firebase = optional(&firebase),
        identity = identity.to_string(),
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("Cargo sets OUT_DIR"));
    fs::write(out.join("gateway_target.rs"), table).expect("write gateway_target.rs");
}
