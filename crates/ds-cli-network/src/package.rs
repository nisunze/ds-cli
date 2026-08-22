//! Reading a `.dsgrid` package, once.
//!
//! Every command in this domain starts by turning a path into bytes and then
//! into either a manifest or a decoded snapshot. Doing that in one place is
//! not only less code — it is the only way the domain's refusals stay
//! identical. A caller who learns `model_not_found` from `ds network inspect`
//! must get the same code, with the same remedy, from `ds network validate`.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_grid_exchange::dsgrid;
use ds_grid_exchange::package::{GridPackage, PackageManifest, unpack};
use serde_json::json;

/// Refuse a file larger than this before reading it. A `.dsgrid` is a zip of
/// compressed Arrow tables; a real one is megabytes. The bound exists so a
/// mistyped path at a disk image fails in a millisecond with a typed reason
/// instead of exhausting memory.
pub const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

/// The refusals every command in this domain shares. Declared here as data so
/// a command can splice them into its own list rather than restating them —
/// a restatement is a second description, and those drift.
pub const SHARED_REFUSALS: &[ds_cli_contract::spec::Refusal] = &[
    ds_cli_contract::spec::Refusal {
        code: "model_not_found",
        when: "the path does not exist or is not a file",
        remedy: "check the path; --model takes a file, not a directory",
    },
    ds_cli_contract::spec::Refusal {
        code: "model_too_large",
        when: "the file is above the 512 MiB read bound",
        remedy: "confirm the file is a .dsgrid package and not a disk image",
    },
    ds_cli_contract::spec::Refusal {
        code: "model_unreadable",
        when: "the file exists but cannot be read",
        remedy: "check file permissions",
    },
    ds_cli_contract::spec::Refusal {
        code: "not_a_dsgrid_package",
        when: "the bytes are not a readable .dsgrid container",
        remedy: "a .dsgrid is a zip containing manifest.json; convert other formats first",
    },
];

pub fn read_bytes(raw_path: &str) -> Result<Vec<u8>, Failure> {
    let path = Path::new(raw_path);
    let metadata = std::fs::metadata(path).map_err(|error| {
        Failure::invalid("model_not_found", format!("cannot read `{raw_path}`"))
            .remedy("check the path; --model takes a .dsgrid file")
            .detail(json!({ "detail": error.kind().to_string() }))
    })?;

    if !metadata.is_file() {
        return Err(
            Failure::invalid("model_not_found", format!("`{raw_path}` is not a file"))
                .remedy("--model takes a .dsgrid file, not a directory"),
        );
    }

    if metadata.len() > MAX_PACKAGE_BYTES {
        return Err(Failure::invalid(
            "model_too_large",
            format!("`{raw_path}` is above the read bound"),
        )
        .remedy("confirm this is a .dsgrid package")
        .detail(json!({ "byte_len": metadata.len(), "max_byte_len": MAX_PACKAGE_BYTES })));
    }

    std::fs::read(path).map_err(|error| {
        Failure::failed("model_unreadable", format!("cannot read `{raw_path}`"))
            .remedy("check file permissions")
            .detail(json!({ "detail": error.kind().to_string() }))
    })
}

/// The cheap read: the package manifest, with no Arrow table decoded.
pub fn read_manifest(raw_path: &str, bytes: &[u8]) -> Result<PackageManifest, Failure> {
    let inspection = dsgrid::inspect(bytes).map_err(|error| {
        Failure::invalid(
            "not_a_dsgrid_package",
            format!("`{raw_path}` is not a readable .dsgrid package"),
        )
        .remedy("a .dsgrid is a zip containing manifest.json")
        .detail(json!({ "detail": error.to_string() }))
    })?;

    if !inspection.readable {
        return Err(Failure::invalid(
            "not_a_dsgrid_package",
            format!("`{raw_path}` has no package manifest"),
        )
        .remedy("re-export the model as .dsgrid"));
    }

    serde_json::from_value(
        inspection
            .detail
            .get("manifest")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )
    .map_err(|error| {
        Failure::failed(
            "manifest_unreadable",
            "the package manifest does not match this build's schema",
        )
        .remedy("rebuild the package with a matching ds-network release")
        .detail(json!({ "detail": error.to_string() }))
    })
}

/// The expensive read: full verification and decode.
///
/// `unpack` verifies every member against the manifest's attestations before
/// returning, so a package that decodes here has already proved its bytes
/// match what it claims. That is why `ds network validate` can report the
/// container and the model as two separate answers.
pub fn decode(raw_path: &str, bytes: &[u8]) -> Result<GridPackage, Failure> {
    unpack(bytes).map_err(|error| {
        Failure::failed(
            "package_decode_failed",
            format!("`{raw_path}` did not decode"),
        )
        .remedy("the package is damaged or predates this schema; re-export it")
        .detail(json!({ "detail": error.to_string() }))
    })
}

/// The canonical serialized token for a table kind — taken from the model
/// crate's own serde representation rather than re-spelled here, so a table
/// renamed upstream is renamed in this output too.
pub fn table_token(kind: ds_grid_model::TableKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{kind:?}"))
}

/// Take at most `limit`, reporting how many were withheld. Truncation is
/// always visible: a silently shortened list reads as a complete one.
pub fn take<T>(mut items: Vec<T>, limit: usize) -> (Vec<T>, usize) {
    if items.len() <= limit {
        return (items, 0);
    }
    let withheld = items.len() - limit;
    items.truncate(limit);
    (items, withheld)
}

pub const MAX_LIMIT: usize = 5_000;
pub const DEFAULT_LIMIT: &str = "50";

pub fn parse_limit(raw: Option<&str>) -> Result<usize, Failure> {
    let raw = raw.unwrap_or(DEFAULT_LIMIT);
    let parsed: usize = raw.parse().map_err(|_| {
        Failure::invalid("invalid_limit", "--limit must be a whole number")
            .remedy(format!("pass 1..{MAX_LIMIT}"))
    })?;
    if parsed == 0 || parsed > MAX_LIMIT {
        return Err(
            Failure::invalid("invalid_limit", "--limit is outside its accepted range")
                .remedy(format!("pass 1..{MAX_LIMIT}"))
                .detail(json!({ "given": parsed, "min": 1, "max": MAX_LIMIT })),
        );
    }
    Ok(parsed)
}
