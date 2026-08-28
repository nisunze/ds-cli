//! `ds pls backup-create` — exact-byte framing of one portable workspace.
//!
//! `ds-io` owns the native container format, path validation, directory
//! records, compression and self-readback. This adapter owns only bounded
//! filesystem collection, stable discovery metadata and the durable output
//! receipt.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_io::{
    PLS_CADD_DETERMINISTIC_BAK_TIMESTAMP, PLS_CADD_DETERMINISTIC_BAK_USER,
    PLS_CADD_WRITE_BASELINE_VERSION, PlsCaddBackupMemberWrite,
    pls_cadd_write_workspace_backup_container,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::workspace_path;

pub static COMMAND: Command = Command {
    id: "pls.backup-create",
    path: &["pls", "backup-create"],
    contract: 1,
    summary: "Create an exact-byte native backup from a closed portable workspace.",
    purpose: "Reads one already reference-ready PLS-CADD workspace without moving or rewriting it, validates its typed member paths through the native ds-io writer, frames every file into one deterministic compressed .bak, and self-reads the result before publishing it. It performs no member conversion, path healing, solver work or native acceptance. Fresh PLS-CADD Restore and reopen remain mandatory before submission.",
    chapter: Chapter::PlsCadd,
    effect: Effect::ArtifactWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "workspace",
            "<dir>",
            "Closed reference-ready PLS-CADD workspace.",
        )
        .required(),
        Arg::value(
            "out",
            "<new.bak>",
            "Absent backup path outside the workspace.",
        )
        .required(),
    ],
    output: "The source snapshot digest, file-member count, backup byte length and SHA-256, exact-byte preservation verdict, and an explicit native Restore/reopen gate.",
    examples: &[Example {
        command: "ds pls backup-create --workspace './PLS-CADD WORKSPACE' --out ./submission.bak --yes --output json",
        note: "Run reference-closure first; this command never claims native Restore acceptance.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "confirmation_required",
            when: "--yes was not supplied",
            remedy: "review the closed source and absent output path, then repeat with --yes",
        },
        Refusal {
            code: "workspace_not_found",
            when: "--workspace is not a directory",
            remedy: "pass the closed PLS-CADD workspace root",
        },
        Refusal {
            code: "workspace_open",
            when: "the workspace carries a PLS-CADD lock or DS open marker",
            remedy: "close PLS-CADD and retry against the unchanged workspace",
        },
        Refusal {
            code: "workspace_read_failed",
            when: "a workspace directory or member cannot be read",
            remedy: "check permissions and preserve the source unchanged",
        },
        Refusal {
            code: "workspace_member_unsafe",
            when: "a member is a symlink, reparse point, non-file, or has a non-portable path",
            remedy: "use a closed self-contained workspace with ordinary files and typed directories",
        },
        Refusal {
            code: "workspace_empty",
            when: "the workspace contains no files",
            remedy: "pass the actual PLS-CADD workspace root",
        },
        Refusal {
            code: "workspace_changed",
            when: "the source inventory or bytes change while the backup is being framed",
            remedy: "close every writer and retry against a stable workspace",
        },
        Refusal {
            code: "output_exists",
            when: "--out already exists",
            remedy: "choose a new immutable .bak path",
        },
        Refusal {
            code: "output_invalid",
            when: "--out is not a .bak path outside the source workspace with an existing parent",
            remedy: "choose an absent .bak in an existing directory outside the workspace",
        },
        Refusal {
            code: "backup_frame_failed",
            when: "the native writer rejects the workspace or its self-readback",
            remedy: "read detail, repair reference/path identity in a new workspace, and retry",
        },
        Refusal {
            code: "output_write_failed",
            when: "the new backup cannot be created and synchronized",
            remedy: "choose a writable absent path and check free space",
        },
    ],
    reference: Some("docs/reference/pls.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let root = workspace_path(inputs.require("workspace")?)?;
    ensure_workspace_closed(&root)?;
    let output = checked_output(&root, inputs.require("out")?)?;

    let mut members = collect_workspace(&root)?;
    if members.is_empty() {
        return Err(
            Failure::invalid("workspace_empty", "the workspace contains no files")
                .remedy("pass the actual PLS-CADD workspace root"),
        );
    }
    sort_members(&mut members);
    let source_snapshot_sha256 = snapshot_digest(&members);

    let archive_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            Failure::invalid("output_invalid", "the output leaf is not valid UTF-8")
                .remedy("choose an ordinary .bak filename")
        })?;
    let backup = pls_cadd_write_workspace_backup_container(
        &members,
        archive_name,
        PLS_CADD_WRITE_BASELINE_VERSION,
        PLS_CADD_DETERMINISTIC_BAK_USER,
        PLS_CADD_DETERMINISTIC_BAK_TIMESTAMP,
    )
    .map_err(|detail| {
        Failure::failed("backup_frame_failed", "the native backup writer refused")
            .remedy("read detail and repair the portable workspace identity")
            .detail(json!({ "detail": detail }))
    })?;

    let mut after = collect_workspace(&root)?;
    sort_members(&mut after);
    if snapshot_digest(&after) != source_snapshot_sha256 {
        return Err(Failure::conflict(
            "workspace_changed",
            "the source workspace changed while the backup was being framed",
        )
        .remedy("close every writer and retry against a stable workspace"));
    }

    write_new(&output, &backup)?;
    Ok(json!({
        "workspace": root.display().to_string(),
        "output": output.display().to_string(),
        "file_members": members.len(),
        "source_snapshot_sha256": source_snapshot_sha256,
        "byte_len": backup.len(),
        "sha256": format!("sha256:{:x}", Sha256::digest(&backup)),
        "member_bytes_preserved": true,
        "path_healing_performed": false,
        "native_restore_reopen_required": true,
        "native_restore_reopen_accepted": false,
    }))
}

fn ensure_workspace_closed(root: &Path) -> Result<(), Failure> {
    if root.join(".ds-workspace-open").exists() {
        return Err(Failure::conflict(
            "workspace_open",
            "the workspace carries the DS open marker",
        )
        .remedy("close PLS-CADD and retry against the unchanged workspace"));
    }
    Ok(())
}

fn checked_output(root: &Path, raw: &str) -> Result<PathBuf, Failure> {
    let output = PathBuf::from(raw);
    if output.exists() {
        return Err(
            Failure::conflict("output_exists", format!("`{raw}` already exists"))
                .remedy("choose a new immutable .bak path"),
        );
    }
    if !output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bak"))
    {
        return Err(
            Failure::invalid("output_invalid", "--out must have a .bak extension")
                .remedy("choose an absent .bak path"),
        );
    }
    let absolute = if output.is_absolute() {
        output
    } else {
        std::env::current_dir()
            .map_err(|error| Failure::failed("output_write_failed", error.to_string()))?
            .join(output)
    };
    let parent = absolute.parent().unwrap_or_else(|| Path::new("."));
    let parent = parent.canonicalize().map_err(|error| {
        Failure::invalid(
            "output_invalid",
            format!("output parent {} is unavailable: {error}", parent.display()),
        )
        .remedy("choose an existing writable output directory")
    })?;
    if parent.starts_with(root) {
        return Err(Failure::invalid(
            "output_invalid",
            "the backup cannot be created inside its source workspace",
        )
        .remedy("choose an absent .bak path outside the workspace"));
    }
    Ok(absolute)
}

fn collect_workspace(root: &Path) -> Result<Vec<PlsCaddBackupMemberWrite>, Failure> {
    let mut members = Vec::new();
    collect_directory(root, root, &mut members)?;
    Ok(members)
}

fn collect_directory(
    root: &Path,
    directory: &Path,
    members: &mut Vec<PlsCaddBackupMemberWrite>,
) -> Result<(), Failure> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|error| workspace_read_error(directory, error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| workspace_read_error(directory, error))?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());
    for entry in entries {
        let path = entry.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|error| workspace_read_error(&path, error))?;
        if is_link_or_reparse(&metadata) {
            return Err(Failure::invalid(
                "workspace_member_unsafe",
                format!(
                    "workspace member is a link or reparse point: {}",
                    path.display()
                ),
            )
            .remedy("use a self-contained workspace with ordinary directories and files"));
        }
        if metadata.is_dir() {
            collect_directory(root, &path, members)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(Failure::invalid(
                "workspace_member_unsafe",
                format!("workspace member is not a regular file: {}", path.display()),
            )
            .remedy("remove the non-file member from a new portable workspace"));
        }
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("lock"))
        {
            return Err(Failure::conflict(
                "workspace_open",
                format!("workspace lock is present: {}", path.display()),
            )
            .remedy("close PLS-CADD and retry against the unchanged workspace"));
        }
        let relative = path.strip_prefix(root).map_err(|error| {
            Failure::invalid("workspace_member_unsafe", error.to_string())
                .remedy("use a self-contained workspace")
        })?;
        let relative = relative.to_str().ok_or_else(|| {
            Failure::invalid(
                "workspace_member_unsafe",
                format!("workspace path is not valid UTF-8: {}", path.display()),
            )
            .remedy("use portable UTF-8 member names")
        })?;
        let body = std::fs::read(&path).map_err(|error| workspace_read_error(&path, error))?;
        members.push(PlsCaddBackupMemberWrite {
            filename: relative.replace('\\', "/"),
            body,
        });
    }
    Ok(())
}

fn workspace_read_error(path: &Path, error: std::io::Error) -> Failure {
    Failure::failed(
        "workspace_read_failed",
        format!("could not read {}", path.display()),
    )
    .remedy("check permissions and preserve the source unchanged")
    .detail(json!({ "detail": error.kind().to_string() }))
}

fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn sort_members(members: &mut [PlsCaddBackupMemberWrite]) {
    members.sort_by(|left, right| {
        left.filename
            .to_ascii_lowercase()
            .cmp(&right.filename.to_ascii_lowercase())
            .then_with(|| left.filename.cmp(&right.filename))
    });
}

fn snapshot_digest(members: &[PlsCaddBackupMemberWrite]) -> String {
    let mut digest = Sha256::new();
    for member in members {
        digest.update((member.filename.len() as u64).to_le_bytes());
        digest.update(member.filename.as_bytes());
        digest.update((member.body.len() as u64).to_le_bytes());
        digest.update(&member.body);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn write_new(output: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| output_write_error(output, error))?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(error) = result {
        drop(file);
        let _ = std::fs::remove_file(output);
        return Err(output_write_error(output, error));
    }
    Ok(())
}

fn output_write_error(path: &Path, error: std::io::Error) -> Failure {
    Failure::failed(
        "output_write_failed",
        format!("could not write {}", path.display()),
    )
    .remedy("choose a writable absent path and check free space")
    .detail(json!({ "detail": error.kind().to_string() }))
}

pub fn render(data: &Value) -> String {
    format!(
        "PLS-CADD backup created\n  {} members · {} bytes\n  {}\n  native Restore/reopen required\n",
        data["file_members"],
        data["byte_len"],
        data["output"].as_str().unwrap_or(""),
    )
}
