//! Writing one new file, never over an existing one.
//!
//! Two commands in this domain produce a durable artifact: `apply` writes a
//! new `.dsgrid` revision, and `project` writes a complete projection
//! document. Both must refuse an existing path with the same code and the
//! same remedy — a caller who learned `output_exists` from one has learned it
//! from the other — and both must leave no partial file behind when a write
//! fails halfway. That is one policy, so it lives in one place.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use serde_json::json;
use sha2::{Digest, Sha256};

/// Refuse an output path before any work is done, so an expensive projection
/// or apply is not computed only to be discarded at the write.
pub fn validate_output_path(raw_path: &str) -> Result<(), Failure> {
    let path = Path::new(raw_path);
    if path.exists() {
        return Err(
            Failure::conflict("output_exists", format!("`{raw_path}` already exists"))
                .remedy("choose a new output path; this domain never overwrites"),
        );
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if parent.is_some_and(|parent| !parent.is_dir()) {
        return Err(Failure::invalid(
            "output_parent_missing",
            format!("the parent of `{raw_path}` does not exist"),
        )
        .remedy("create the intended output directory, then retry"));
    }
    Ok(())
}

/// Create and fully write one new file. A path that already exists refuses;
/// a write that fails partway removes what it wrote, because a truncated
/// artifact that looks complete is worse than no artifact at all.
pub fn write_new(raw_path: &str, bytes: &[u8]) -> Result<(), Failure> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(raw_path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Failure::conflict("output_exists", format!("`{raw_path}` already exists"))
                    .remedy("choose a new output path; this domain never overwrites")
            } else {
                Failure::failed("output_unwritable", format!("cannot create `{raw_path}`"))
                    .remedy("choose a new writable output path")
                    .detail(json!({ "detail": error.kind().to_string() }))
            }
        })?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(raw_path);
        return Err(Failure::failed(
            "output_unwritable",
            format!("could not finish writing `{raw_path}`"),
        )
        .remedy("check free space and permissions; the partial file was removed")
        .detail(json!({ "detail": error.kind().to_string() })));
    }
    Ok(())
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
