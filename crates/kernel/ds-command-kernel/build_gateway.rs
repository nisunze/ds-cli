//! The gateways a Kernel build speaks to, decided when it is built.
//!
//! A page build's lane is a cargo feature (`lane-canary` or
//! `lane-production`); the local development stack adds `topology-local`,
//! where the page's own origin proxies the gateway. A native build (Core)
//! serves every release channel `release/wasm-unit.json` declares in
//! `releaseTargets`, each through its target's lane. The gateway origins, the
//! platform API key and the Firebase project and key come from the project's
//! build environment, never from a repository:
//!
//!   DS_GATEWAY_URL_CANARY, DS_GATEWAY_URL_PRODUCTION, DS_PLATFORM_API_KEY,
//!   DS_FIREBASE_PROJECT_ID, DS_FIREBASE_API_KEY, DS_GOOGLE_CLIENT_ID (optional:
//!   the OAuth client of Google's redirect sign-in; without it a page refuses
//!   Google sign-in as unconfigured)
//!
//! and a packaged build names the Kernel revision it was pinned to in
//! DS_RELEASE_PIN_DS_COMMAND_KERNEL; every build names the digest of the
//! routing descriptor (routing/operations.json) it compiled.
//!
//! A deployed page lane refuses to build without its gateway origin and the
//! key, and every page build without its Firebase project and key; a native
//! build leaves a channel whose gateway was not named unserved.

use sha2::{Digest, Sha256};
use std::{env, fs, path::Path, path::PathBuf};

const VARS: [&str; 7] = [
    "DS_GATEWAY_URL_CANARY",
    "DS_GATEWAY_URL_PRODUCTION",
    "DS_PLATFORM_API_KEY",
    "DS_FIREBASE_PROJECT_ID",
    "DS_FIREBASE_API_KEY",
    "DS_GOOGLE_CLIENT_ID",
    "DS_RELEASE_PIN_DS_COMMAND_KERNEL",
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

/// Each release channel and the lane its declared target speaks to, from
/// `releaseTargets` and `targets` of release/wasm-unit.json.
fn release_channels(root: &Path) -> Vec<(String, &'static str)> {
    let path = root.join("release/wasm-unit.json");
    println!("cargo:rerun-if-changed={}", path.display());
    let unit: serde_json::Value = serde_json::from_slice(
        &fs::read(&path).unwrap_or_else(|_| fail("release/wasm-unit.json is unreadable")),
    )
    .unwrap_or_else(|_| fail("release/wasm-unit.json is not JSON"));
    let channels = unit["releaseTargets"]
        .as_object()
        .unwrap_or_else(|| fail("release/wasm-unit.json declares no releaseTargets"));
    channels
        .iter()
        .map(|(channel, target)| {
            let name = target.as_str().unwrap_or_default();
            let features = unit["targets"][name]["features"]
                .as_array()
                .unwrap_or_else(|| fail(format!("release channel {channel} names no target")));
            let has = |feature: &str| features.iter().any(|value| value == feature);
            let lane = match (has("lane-canary"), has("lane-production")) {
                _ if has("topology-local") => {
                    fail(format!("release channel {channel} names a local target"))
                }
                (true, false) => "canary",
                (false, true) => "production",
                _ => fail(format!("release target {name} must name exactly one lane")),
            };
            (channel.clone(), lane)
        })
        .collect()
}

/// The digest of the routing descriptor (routing/operations.json) this build
/// compiled: the provenance a native client profile names.
fn routing_sha256(root: &Path) -> String {
    let path = root.join("routing/operations.json");
    println!("cargo:rerun-if-changed={}", path.display());
    let bytes = fs::read(&path).unwrap_or_else(|_| fail("routing/operations.json is unreadable"));
    format!("{:x}", Sha256::digest(&bytes))
}

/// The Kernel revision a packaged build was pinned to; a development build
/// says it was not.
fn source_revision() -> String {
    match var("DS_RELEASE_PIN_DS_COMMAND_KERNEL") {
        Some(pin)
            if pin.len() == 40
                && pin
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) =>
        {
            pin
        }
        Some(_) => fail("DS_RELEASE_PIN_DS_COMMAND_KERNEL must be one lowercase 40-hex SHA"),
        None => "unpinned".to_owned(),
    }
}

/// Write `gateway_target.rs` into OUT_DIR for `src/gateway_target.rs`.
pub fn emit(root: &Path) {
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
    let firebase_key = var("DS_FIREBASE_API_KEY");
    let google_client = var("DS_GOOGLE_CLIENT_ID");
    if google_client.as_deref().is_some_and(|id| {
        !id.ends_with(".apps.googleusercontent.com")
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'))
    }) {
        fail("DS_GOOGLE_CLIENT_ID must be one OAuth client id (<id>.apps.googleusercontent.com)");
    }
    let channels = release_channels(root);
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
    if lane.is_some() && (firebase.is_none() || firebase_key.is_none()) {
        fail("a page build needs DS_FIREBASE_PROJECT_ID and DS_FIREBASE_API_KEY: its audience and its sign-in");
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
        "google_client_id": google_client,
    });
    let channels = channels
        .iter()
        .map(|(channel, lane)| format!("({channel:?}, {lane:?})"))
        .collect::<Vec<_>>()
        .join(", ");
    let table = format!(
        "pub const LANE: Option<&str> = {lane};\n\
         pub const LOCAL: bool = {local};\n\
         pub const GATEWAY_ORIGIN: Option<&str> = {gateway};\n\
         pub const RELEASE_ORIGIN_CANARY: Option<&str> = {canary};\n\
         pub const RELEASE_ORIGIN_PRODUCTION: Option<&str> = {production};\n\
         pub const PLATFORM_API_KEY: Option<&str> = {key};\n\
         pub const FIREBASE_PROJECT: Option<&str> = {firebase};\n\
         pub const FIREBASE_API_KEY: Option<&str> = {firebase_key};\n\
         pub const GOOGLE_CLIENT_ID: Option<&str> = {google_client};\n\
         pub const RELEASE_CHANNELS: &[(&str, &str)] = &[{channels}];\n\
         pub const SOURCE_REVISION: &str = {revision:?};\n\
         pub const ROUTING_SHA256: &str = {routing:?};\n\
         pub const IDENTITY: &str = {identity:?};\n",
        lane = optional(&lane.map(str::to_owned)),
        gateway = optional(&gateway),
        canary = optional(&canary),
        production = optional(&production),
        key = optional(&key),
        firebase = optional(&firebase),
        firebase_key = optional(&firebase_key),
        google_client = optional(&google_client),
        revision = source_revision(),
        routing = routing_sha256(root),
        identity = identity.to_string(),
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("Cargo sets OUT_DIR"));
    fs::write(out.join("gateway_target.rs"), table).expect("write gateway_target.rs");
}
