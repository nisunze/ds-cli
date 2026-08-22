//! Turning `--source` paths into a `SourceSet`, once.
//!
//! All three commands in this domain start the same way: read the operator's
//! paths into bytes and hand them to the engine as one `SourceSet`. Doing
//! that in one place is not only less code — it is the only way the domain's
//! refusals stay identical. A caller who learns `source_too_large` from
//! `ds dsgrid-exchange inspect` must get the same code, with the same remedy,
//! from `plan` and from `convert`; otherwise "inspect first, then convert"
//! stops being a reliable sequence.
//!
//! It is also the only place a directory is walked, which is what makes the
//! digest stable — see `read_folder`.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_grid_exchange::conversion::{SourceCandidate, SourceSet};
use serde_json::json;

/// A folder source is read whole. The bound is the same one the reference
/// closure task applies to a workspace, for the same reason: a mistyped path
/// at a large tree should fail in a moment, not after reading it.
pub const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_FILES: usize = 4_096;

/// The `--source` input, declared once. Every command in this domain takes
/// exactly this argument with exactly this help line, so the three screens
/// describe one concept rather than three similar ones.
pub const SOURCE_ARG: Arg = Arg::repeated(
    "source",
    "<path>",
    "A file, or a directory read as one folder source. Repeatable.",
)
.required();

/// The refusals every command in this domain shares. Declared here as data so
/// a command can splice them into its own list rather than restating them —
/// a restatement is a second description, and those drift.
pub const SHARED_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "source_not_found",
        when: "a --source path does not exist",
        remedy: "check each path; a directory is read as one folder source",
    },
    Refusal {
        code: "source_too_large",
        when: "the sources exceed the 512 MiB or 4096-file read bound",
        remedy: "convert a narrower subtree",
    },
    Refusal {
        code: "source_unreadable",
        when: "a source exists but cannot be read",
        remedy: "check file permissions",
    },
];

/// What was read, alongside the set itself. The byte and file totals are
/// reported rather than discarded: they are the cost model a caller needs to
/// decide whether the next call is worth making.
pub struct Loaded {
    pub sources: SourceSet,
    pub byte_len: u64,
    pub file_count: usize,
}

pub fn load(raw_sources: &[String]) -> Result<Loaded, Failure> {
    let mut candidates = Vec::with_capacity(raw_sources.len());
    let mut byte_len: u64 = 0;
    let mut file_count: usize = 0;

    for raw in raw_sources {
        let path = Path::new(raw.as_str());
        let metadata = std::fs::metadata(path).map_err(|error| {
            Failure::invalid("source_not_found", format!("cannot read `{raw}`"))
                .remedy("check each path; a directory is read as one folder source")
                .detail(json!({ "detail": error.kind().to_string() }))
        })?;

        if metadata.is_dir() {
            let members = read_folder(path, &mut byte_len, &mut file_count)?;
            candidates.push(SourceCandidate::folder(display_name(path), members));
        } else {
            file_count += 1;
            byte_len += metadata.len();
            check_bounds(byte_len, file_count)?;
            let bytes = std::fs::read(path).map_err(|error| {
                Failure::failed("source_unreadable", format!("cannot read `{raw}`"))
                    .remedy("check file permissions")
                    .detail(json!({ "detail": error.kind().to_string() }))
            })?;
            candidates.push(SourceCandidate::file(display_name(path), bytes));
        }
    }

    Ok(Loaded {
        sources: SourceSet::new(candidates),
        byte_len,
        file_count,
    })
}

/// Read a directory as one folder source, in a deterministic order.
///
/// Order matters: the engine digests the member list, so two runs over the
/// same tree must produce the same digest. Directory iteration order is not
/// guaranteed by the OS, so it is sorted here. A conversion whose plan id
/// changed because a filesystem returned entries in a different order would
/// make digest pinning worthless.
fn read_folder(
    root: &Path,
    byte_len: &mut u64,
    file_count: &mut usize,
) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let mut members = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut paths: Vec<PathBuf> = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|error| {
            Failure::failed(
                "source_unreadable",
                format!("cannot list `{}`", dir.display()),
            )
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
            Failure::failed(
                "source_unreadable",
                format!("cannot read `{}`", path.display()),
            )
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
            Failure::failed(
                "source_unreadable",
                format!("cannot read `{}`", path.display()),
            )
            .remedy("check file permissions")
            .detail(json!({ "detail": error.kind().to_string() }))
        })?;
        members.push((relative, bytes));
    }

    Ok(members)
}

fn check_bounds(byte_len: u64, file_count: usize) -> Result<(), Failure> {
    if byte_len > MAX_TOTAL_BYTES || file_count > MAX_FILES {
        return Err(
            Failure::invalid("source_too_large", "the sources exceed the read bound")
                .remedy("convert a narrower subtree")
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

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
