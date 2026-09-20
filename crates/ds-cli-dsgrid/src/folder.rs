//! Reading a directory as one PLS-CADD folder source, once.
//!
//! Three commands read a workspace folder into `(relative path, bytes)`
//! members: `ds dsgrid-exchange inspect|plan|convert` classify and convert
//! it, `ds dsgrid model link` pins it, and `ds dsgrid-exchange sync` writes
//! back into it. They must all read the same members in the same order,
//! because the exchange digests the member list and the digest `inspect`
//! prints is the digest `link` pins and `sync` checks. One walk, one order,
//! one bound — here.
//!
//! Order matters: directory iteration order is not guaranteed by the OS, so
//! the paths are sorted. A digest that changed because a filesystem returned
//! entries in a different order would make digest pinning worthless.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use serde_json::json;

/// A folder source is read whole. The bound is the same one the reference
/// closure task applies to a workspace, for the same reason: a mistyped path
/// at a large tree should fail in a moment, not after reading it.
pub const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_FILES: usize = 4_096;

pub const TOO_LARGE: Refusal = Refusal {
    code: "source_too_large",
    when: "the sources exceed the 512 MiB or 4096-file read bound",
    remedy: "convert a narrower subtree",
};
pub const UNREADABLE: Refusal = Refusal {
    code: "source_unreadable",
    when: "a source exists but cannot be read",
    remedy: "check file permissions",
};

/// Read a directory as one folder source, in a deterministic order.
///
/// `byte_len` and `file_count` accumulate across calls so a caller reading
/// several sources holds them all to one bound.
pub fn read_folder(
    root: &Path,
    byte_len: &mut u64,
    file_count: &mut usize,
) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let mut members = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut paths: Vec<PathBuf> = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|error| {
            Failure::failed(UNREADABLE.code, format!("cannot list `{}`", dir.display()))
                .remedy("check directory permissions")
                .detail(json!({ "detail": error.kind().to_string() }))
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                paths.push(path);
            }
        }
    }
    paths.sort();

    for path in paths {
        let metadata = std::fs::metadata(&path).map_err(|error| {
            Failure::failed(UNREADABLE.code, format!("cannot read `{}`", path.display()))
                .detail(json!({ "detail": error.kind().to_string() }))
        })?;
        *file_count += 1;
        *byte_len += metadata.len();
        check_bounds(*byte_len, *file_count)?;

        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&path).map_err(|error| {
            Failure::failed(UNREADABLE.code, format!("cannot read `{}`", path.display()))
                .remedy(UNREADABLE.remedy)
                .detail(json!({ "detail": error.kind().to_string() }))
        })?;
        members.push((relative, bytes));
    }

    Ok(members)
}

pub fn check_bounds(byte_len: u64, file_count: usize) -> Result<(), Failure> {
    if byte_len > MAX_TOTAL_BYTES || file_count > MAX_FILES {
        return Err(
            Failure::invalid(TOO_LARGE.code, "the sources exceed the read bound")
                .remedy(TOO_LARGE.remedy)
                .detail(json!({
                    "byte_len": byte_len,
                    "files": file_count,
                    "max_byte_len": MAX_TOTAL_BYTES,
                    "max_files": MAX_FILES,
                })),
        );
    }
    Ok(())
}

/// Whether a path sits on a volume that streams or mirrors its bytes on
/// demand (Google Drive, OneDrive, a mapped or UNC share), where a digest
/// pinned now can change under the pin and reads can stall. Named by the
/// path only — a mount table is not consulted — so this is a warning the
/// operator reads, never a refusal.
pub fn streamed_volume_hint(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    let lower = text.to_ascii_lowercase();
    let reason = if text.starts_with("\\\\") || text.starts_with("//") {
        "a UNC network share"
    } else if lower.contains("shared drives")
        || lower.contains("my drive")
        || lower.contains("google drive")
    {
        "a Google Drive stream"
    } else if lower.contains("onedrive") || lower.contains("dropbox") {
        "a cloud-synced folder"
    } else if lower.starts_with("g:\\") || lower.starts_with("g:/") {
        "drive G:, which is a streamed volume on the design workstations"
    } else {
        return None;
    };
    Some(format!(
        "`{text}` is on {reason}: its bytes can change under a pinned digest and reads can stall; copy the workspace to a local disk before converting or syncing"
    ))
}
