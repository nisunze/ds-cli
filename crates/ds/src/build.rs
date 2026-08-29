//! Build identity, stamped at compile time by `build.rs`.
//!
//! Packaging runs `ds version --output json` against the staged executable
//! and asserts these fields against its own pin. That check is the reason
//! every value here comes from the compiler's environment rather than from a
//! constant someone has to remember to bump.

use serde_json::{Value, json};

pub const PRODUCT: &str = "ds";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SOURCE_SHA: &str = env!("DS_BUILD_SHA");
pub const NATIVE_CLIENT_CORE_SHA: &str = env!("DS_NATIVE_CLIENT_CORE_SHA");
pub const TARGET: &str = env!("DS_BUILD_TARGET");
pub const PROFILE: &str = env!("DS_BUILD_PROFILE");

/// Whether the working tree carried uncommitted tracked changes. A release
/// build must be false; a development build says so rather than pretending.
pub fn dirty() -> bool {
    env!("DS_BUILD_DIRTY") == "1"
}

pub fn identity() -> Value {
    json!({
        "product": PRODUCT,
        "version": VERSION,
        "source_sha": SOURCE_SHA,
        "native_client_core_source_sha": NATIVE_CLIENT_CORE_SHA,
        "dirty": dirty(),
        "target": TARGET,
        "profile": PROFILE,
        "envelope": ds_cli_contract::outcome::ENVELOPE_VERSION,
    })
}
