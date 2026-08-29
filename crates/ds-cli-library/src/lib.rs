//! `ds library` — immutable parallel engineering-library versions.
//!
//! The linked ds-network engine owns manifests, one-way PLS-CADD ingestion,
//! digests and release verification. This crate owns bounded local transport:
//! reading explicit paths and materializing new paths without overwrite.

pub mod catalog;
pub mod global_catalog;
pub mod open;
pub mod pack;
pub mod resolve_native;
pub mod seed;
pub mod unpack;
pub mod verify;

use std::fs;
use std::path::Path;

use ds_cli_contract::Failure;
use ds_cli_contract::outcome::ExitClass;
use ds_cli_contract::spec::Domain;
use sha2::{Digest, Sha256};

pub static DOMAIN: Domain = Domain {
    id: "library",
    summary: "Engineering libraries: verify, store, pack, seed.",
    commands: &[
        &verify::COMMAND,
        &open::COMMAND,
        &catalog::COMMAND,
        &global_catalog::READ_COMMAND,
        &global_catalog::WRITE_COMMAND,
        &global_catalog::FORK_COMMAND,
        &global_catalog::UPLOAD_COMMAND,
        &global_catalog::PUBLISH_LIBRARY_COMMAND,
        &global_catalog::PUBLISH_EXAMPLE_COMMAND,
        &global_catalog::LIBRARY_LIFECYCLE_COMMAND,
        &global_catalog::EXAMPLE_LIFECYCLE_COMMAND,
        &pack::COMMAND,
        &unpack::COMMAND,
        &seed::COMMAND,
        &resolve_native::COMMAND,
    ],
};

const MAX_READ_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn read(path: &str) -> Result<Vec<u8>, Failure> {
    let path_ref = Path::new(path);
    let metadata = fs::metadata(path_ref).map_err(|error| {
        Failure::invalid(
            "library_path_not_found",
            format!("`{path}` is not readable"),
        )
        .remedy("check the explicit library/source path")
        .detail(serde_json::json!({ "io": error.kind().to_string() }))
    })?;
    if !metadata.is_file() {
        return Err(Failure::invalid(
            "library_path_not_file",
            format!("`{path}` is not a file"),
        ));
    }
    if metadata.len() > MAX_READ_BYTES {
        return Err(Failure::invalid(
            "library_file_too_large",
            format!("`{path}` exceeds the 2 GiB read bound"),
        ));
    }
    fs::read(path_ref).map_err(|error| {
        Failure::failed("library_read_failed", format!("could not read `{path}`"))
            .detail(serde_json::json!({ "io": error.kind().to_string() }))
    })
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    if path.exists() {
        return Err(Failure::conflict(
            "output_exists",
            format!("`{}` already exists", path.display()),
        )
        .remedy("choose a new immutable output path"));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Failure::failed("output_unwritable", "could not create the output directory")
                .detail(serde_json::json!({ "io": error.kind().to_string() }))
        })?;
    }
    fs::write(path, bytes).map_err(|error| {
        Failure::failed(
            "output_unwritable",
            format!("could not write `{}`", path.display()),
        )
        .detail(serde_json::json!({ "io": error.kind().to_string() }))
    })
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn engine_failure(code: &str, error: impl std::fmt::Display) -> Failure {
    Failure::new(ExitClass::Failed, code, error.to_string())
}
