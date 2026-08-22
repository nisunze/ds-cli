//! `ds tiler` — sealed, fully local PMTiles workspace execution.
//!
//! This is deliberately not part of `ds map`: the map domain operates the
//! paired application's live MapLibre state, while this domain asks the
//! native `ds-vector-tiler` engine to build one deterministic PMTiles artifact
//! from a sealed filesystem workspace. It is also not `dsgrid`: a `.dsgrid`
//! package is a canonical network model, whereas a tiler workspace is an
//! already materialized, hash-pinned GeoJSONL snapshot plus a governed tile
//! policy.
//!
//! The process boundary is closed. The caller gives `ds` one workspace root;
//! this crate names the only engine subcommand in source, accepts no source,
//! output, tool, URL or arbitrary-argument flags, and refuses a result that
//! claims any remote execution. The native engine owns all PMTiles work.

pub mod workspace;

use std::path::{Path, PathBuf};
use std::time::Duration;

use ds_cli_contract::Failure;
use ds_cli_contract::spec::{Availability, Domain};
use ds_cli_exec::{EnvironmentVariable, External};
use serde_json::json;

/// The local native tiler. A packaged desktop puts it beside `ds`; a source
/// tree may set the explicit override. It is never an HTTP endpoint.
pub static DS_VECTOR_TILER: External = External {
    name: "ds-vector-tiler",
    env_override: "DS_VECTOR_TILER_BIN",
    owner: "ds-vector-tiler",
    remedy: "install the ds-vector-tiler desktop binary, or set DS_VECTOR_TILER_BIN to a built executable",
    missing_code: "tiler_engine_missing",
    environment: Some(tiler_environment),
};

/// Local tiling can process a large sealed snapshot and run its native engine.
/// The bound is deliberately finite: a hung native addition must not leave an
/// automation caller waiting indefinitely.
pub const WORKSPACE_TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

pub static DOMAIN: Domain = Domain {
    id: "tiler",
    summary: "Sealed local PMTiles.",
    commands: &[&workspace::COMMAND],
};

/// Availability includes the required native addition as well as the
/// engine itself. This remains filesystem-only: help and `ds doctor` never
/// execute a version probe, but they should not promise a runnable local
/// tiler when the packaged tool set is incomplete.
pub(crate) fn tiler_availability() -> Availability {
    let Some(binary) = DS_VECTOR_TILER.locate() else {
        return DS_VECTOR_TILER.availability();
    };
    match tiler_environment(&binary) {
        Ok(_) => Availability::Available,
        Err(failure) => Availability::unavailable(
            "tiler_addition_missing",
            failure.message(),
            failure.remedy_text().unwrap_or(
                "install the pinned desktop addition beside ds-vector-tiler, or set its exact absolute path",
            ),
        ),
    }
}

/// Bind the one native addition to the tiler child only. The packaged form is
/// two adjacent executables; source/developer use may override Tippecanoe
/// explicitly, but only with an absolute app-owned path. PMTiles conversion is
/// linked Rust code inside `ds-vector-tiler`, not an executable addition.
/// Deliberately do not search PATH: a stale system tool must not silently
/// outrank the packaged addition that the sealed manifest pins.
fn tiler_environment(binary: &Path) -> Result<Vec<EnvironmentVariable>, Failure> {
    Ok(vec![EnvironmentVariable {
        name: "DS_VECTOR_TILER_TIPPECANOE_BIN",
        value: addition_path(binary, "DS_VECTOR_TILER_TIPPECANOE_BIN", "tippecanoe")?
            .into_os_string(),
    }])
}

fn addition_path(binary: &Path, env_name: &'static str, name: &str) -> Result<PathBuf, Failure> {
    let configured = std::env::var_os(env_name).map(PathBuf::from);
    let candidate = match configured {
        Some(path) => {
            if !path.is_absolute() {
                return Err(addition_missing(
                    env_name,
                    "the configured addition path is not absolute",
                ));
            }
            path
        }
        None => {
            let binary = binary.canonicalize().map_err(|_| {
                addition_missing(
                    env_name,
                    "the resolved ds-vector-tiler binary cannot be canonicalized",
                )
            })?;
            let parent = binary.parent().ok_or_else(|| {
                addition_missing(
                    env_name,
                    "the resolved ds-vector-tiler binary has no parent directory",
                )
            })?;
            parent.join(platform_binary_name(name))
        }
    };
    let path = candidate.canonicalize().map_err(|_| {
        addition_missing(
            env_name,
            "the required sibling desktop addition is not installed",
        )
    })?;
    if !path.is_file() {
        return Err(addition_missing(
            env_name,
            "the configured desktop addition is not a file",
        ));
    }
    Ok(path)
}

fn platform_binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn addition_missing(env_name: &'static str, reason: &'static str) -> Failure {
    Failure::unavailable("tiler_addition_missing", reason)
        .remedy(format!(
            "install the pinned desktop addition beside ds-vector-tiler, or set {env_name} to its absolute path"
        ))
        .detail(json!({ "environment": env_name }))
}
