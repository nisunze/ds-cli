//! `ds pls desktop` — PLS-CADD 16.81 itself, driven on its Windows desktop.
//!
//! Every other command in this domain reads or writes PLS files through a
//! typed task. These run the application: native Restore, AutoSag, the six
//! deliverable reports, the plan & profile PDF and the File > Backup of a
//! saved project. The owner of that work is the set of PowerShell drivers
//! proven on the Nyamagabe delivery (ds-work `ea67e9b`), and this module does
//! not re-implement any of it:
//!
//! * [`bundle`] embeds the drivers byte for byte, each with its sha256 pinned
//!   in source, and extracts them into a private temporary folder per call;
//! * `run` starts Windows PowerShell 5.1 on exactly one ds entry script, with
//!   a static switch list and typed values — never a caller-supplied argv;
//! * `failure` turns the one result document an entry writes, and the
//!   drivers' own journals, into a typed refusal;
//! * [`catalog`] reads the dialog catalogue the watcher decides by. Which
//!   dialog is safe to answer is the catalogue's decision, never this code's:
//!   an unknown dialog stops the run.
//!
//! The verbs refuse `windows_only` anywhere else, and `pls_cadd_not_found`
//! where PLS-CADD is not installed at the path every driver pins. `dialogs`
//! is the exception: it only reads the embedded catalogue, so it answers on
//! any host.

pub mod autosag;
pub mod bundle;
pub mod catalog;
pub mod check;
pub mod deliver;
pub mod dialogs;
mod failure;
pub mod qualify;
pub mod reports;
pub mod restore;
mod run;
pub mod sheets_pdf;

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Availability, Refusal};
use serde_json::{Value, json};

/// Where every vendored driver expects PLS-CADD. The restore, backup and close
/// drivers compare a running process's path to this literal, so a PLS-CADD
/// installed anywhere else is not one they will drive.
pub const PLS_CADD_EXECUTABLE: &str = r"C:\Program Files\PLS\pls_cadd\pls_cadd64.exe";

pub const WINDOWS_ONLY: Refusal = Refusal {
    code: "windows_only",
    when: "this host is not Windows; PLS-CADD and its drivers run only there",
    remedy: "run the same ds command on the Windows desktop that has PLS-CADD 16.81",
};
pub const PLS_CADD_NOT_FOUND: Refusal = Refusal {
    code: "pls_cadd_not_found",
    when: r"C:\Program Files\PLS\pls_cadd\pls_cadd64.exe is absent",
    remedy: "install PLS-CADD 16.81 at its default path, or use the desktop that has it",
};
pub const POWERSHELL_NOT_FOUND: Refusal = Refusal {
    code: "powershell_not_found",
    when: "Windows PowerShell 5.1 is absent from System32",
    remedy: "restore the Windows PowerShell 5.1 feature; PowerShell 7 is not a substitute",
};
pub const PLS_CADD_RUNNING: Refusal = Refusal {
    code: "pls_cadd_running",
    when: "PLS-CADD is already running on this desktop",
    remedy: "look at the open PLS-CADD first, close it, then retry into a new folder",
};
pub const PLS_CADD_MISMATCH: Refusal = Refusal {
    code: "pls_cadd_mismatch",
    when: "the installed PLS-CADD is not the pinned 16.81 build",
    remedy: "install the pinned 16.81 build; `ds pls desktop check` shows both digests",
};
pub const OUTPUT_EXISTS: Refusal = Refusal {
    code: "output_exists",
    when: "a folder this run would create already exists",
    remedy: "choose a new folder; every run starts from a fresh one",
};
pub const OUTPUT_PARENT_MISSING: Refusal = Refusal {
    code: "output_parent_missing",
    when: "the parent of a folder this run would create does not exist",
    remedy: "create the parent folder first, or choose one that exists",
};
pub const SYSTEM_DRIVE_REFUSED: Refusal = Refusal {
    code: "system_drive_refused",
    when: "a folder this run would create is on C:",
    remedy: r"put project work on the project Drive, e.g. G:\Shared drives\<project>\Working",
};
pub const SOURCE_NOT_FOUND: Refusal = Refusal {
    code: "source_not_found",
    when: "--bak or --project is not a file",
    remedy: "check the path passed to the flag",
};
pub const PROJECT_INVALID: Refusal = Refusal {
    code: "project_invalid",
    when: "--project is not a .xyz file",
    remedy: "pass the project's .xyz entry point; PLS-CADD opens a project by it",
};
pub const INVALID_DIGEST: Refusal = Refusal {
    code: "invalid_digest",
    when: "--sha256 is not a sha256 digest",
    remedy: "pass sha256:<64 hex> or the 64 hex digits alone",
};
pub const BACKUP_DIGEST_MISMATCH: Refusal = Refusal {
    code: "backup_digest_mismatch",
    when: "the backup's bytes do not match --sha256",
    remedy: "pin the digest of the backup you intend, or pass the intended file",
};
pub const BACKUP_INVALID: Refusal = Refusal {
    code: "backup_invalid",
    when: "the restore driver cannot read the file as a PLS-CADD backup of one project",
    remedy: "pass a native PLSBACKUPFILE or its one-member ZIP; read detail.message",
};
pub const INVALID_ARGUMENT: Refusal = Refusal {
    code: "invalid_argument",
    when: "a value is outside what the driver accepts",
    remedy: "read the message; paths are absolute or relative, names are plain leaves",
};
pub const UNKNOWN_DIALOG: Refusal = Refusal {
    code: "unknown_dialog",
    when: "PLS-CADD showed a dialog the catalogue does not know, and the run stopped",
    remedy: "inspect it in PLS-CADD, record it in the dialog catalogue with its decision (never click through it blind), retry into a new folder",
};
pub const DIALOG_STOP: Refusal = Refusal {
    code: "dialog_stop",
    when: "a catalogued dialog whose decision stops an unattended run came up",
    remedy: "read detail.dialog with `ds pls desktop dialogs --name <dialog>` and repair the project",
};
pub const PLS_CADD_TIMEOUT: Refusal = Refusal {
    code: "pls_cadd_timeout",
    when: "PLS-CADD did not reach the state a driver waited for in time",
    remedy: "inspect PLS-CADD; for a slow report raise --report-timeout; retry into a new folder",
};
pub const RESTORED_TREE_MISMATCH: Refusal = Refusal {
    code: "restored_tree_mismatch",
    when: "the restored files differ from the backup's members",
    remedy: "read detail.message; the backup or the restore folder is not what was intended",
};
pub const WORD_NOT_FOUND: Refusal = Refusal {
    code: "word_not_found",
    when: "Microsoft Word is not registered; report PDFs are made from the RTFs with it",
    remedy: "install Microsoft Word on the desktop before a run that makes report PDFs",
};
pub const DRIVER_FAILED: Refusal = Refusal {
    code: "driver_failed",
    when: "a driver refused for a reason without its own code",
    remedy: "read detail.message and detail.script; PLS-CADD may still be open (detail.pls_cadd_running)",
};
pub const RUN_TIMED_OUT: Refusal = Refusal {
    code: "desktop_run_timed_out",
    when: "the whole run exceeded ds's bound for this verb",
    remedy: "PLS-CADD may still be running: inspect it and close it before retrying",
};
pub const BUNDLE_FAILED: Refusal = Refusal {
    code: "driver_bundle_failed",
    when: "the embedded drivers could not be extracted and verified in a private temporary folder",
    remedy: "check that %TEMP% is writable and has space, then retry",
};
pub const RESULT_UNREADABLE: Refusal = Refusal {
    code: "driver_result_unreadable",
    when: "the run ended without a readable result document or receipt",
    remedy: "read detail.stderr; update ds if its drivers and its reader disagree",
};

/// `--sha256` for a verb that restores a backup. Optional: `ds` computes the
/// digest when it is absent and the driver re-checks it before PLS-CADD sees
/// the file either way.
pub const SHA256_ARG: Arg = Arg::value(
    "sha256",
    "<sha256:hex>",
    "Pin the backup's digest; computed and reported when absent.",
);
pub const SOURCE_ROOT_ARG: Arg = Arg::value(
    "source-root",
    r"<C:\dir>",
    "The backup's original root, for a backup spanning several roots.",
);
pub const PROJECT_FILE_ARG: Arg = Arg::value(
    "project-file",
    "<name.xyz>",
    "The project to open when the backup holds several.",
);
pub const REPORT_TIMEOUT_ARG: Arg = Arg::value(
    "report-timeout",
    "<seconds>",
    "Longest wait for one report (60-14400).",
)
.default("1800");

/// Whether this host can run the drivers, from filesystem metadata only:
/// help and the domain index call this, and it must never start a process.
pub fn availability() -> Availability {
    if !cfg!(windows) {
        return Availability::unavailable(
            WINDOWS_ONLY.code,
            "PLS-CADD and its desktop drivers run only on Windows",
            WINDOWS_ONLY.remedy,
        );
    }
    if !Path::new(PLS_CADD_EXECUTABLE).is_file() {
        return Availability::unavailable(
            PLS_CADD_NOT_FOUND.code,
            format!("PLS-CADD is not installed at {PLS_CADD_EXECUTABLE}"),
            PLS_CADD_NOT_FOUND.remedy,
        );
    }
    if !run::powershell().is_file() {
        return Availability::unavailable(
            POWERSHELL_NOT_FOUND.code,
            format!("{} is absent", run::powershell().display()),
            POWERSHELL_NOT_FOUND.remedy,
        );
    }
    Availability::Available
}

/// An absolute path without resolving links or adding a `\\?\` prefix.
///
/// `canonicalize` would return `\\?\G:\…` on Windows. The drivers type these
/// paths into PLS-CADD's own file and folder pickers, and compare them with
/// `^[Cc]:`; a verbatim prefix defeats both.
fn absolute(raw: &str) -> Result<PathBuf, Failure> {
    std::path::absolute(raw).map_err(|error| {
        Failure::invalid(
            INVALID_ARGUMENT.code,
            format!("`{raw}` cannot be made absolute"),
        )
        .remedy(INVALID_ARGUMENT.remedy)
        .detail(json!({ "detail": error.kind().to_string() }))
    })
}

/// A path as the drivers will receive it: absolute, UTF-8, no verbatim prefix.
fn display(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\")
        .map(str::to_string)
        .unwrap_or(text)
}

/// An existing input file (`--bak`, `--project`), as an absolute path.
fn input_file(raw: &str, flag: &str) -> Result<String, Failure> {
    let path = absolute(raw)?;
    if !path.is_file() {
        return Err(
            Failure::invalid(SOURCE_NOT_FOUND.code, format!("`{raw}` is not a file"))
                .remedy(format!("check the path passed to --{flag}")),
        );
    }
    Ok(display(&path))
}

/// A project's `.xyz` entry point. `pls-launch-project.ps1` refuses any other
/// file; refusing here names the flag before PLS-CADD is started.
fn project_file(raw: &str) -> Result<String, Failure> {
    let project = input_file(raw, "project")?;
    if !has_extension(&project, "xyz") {
        return Err(Failure::invalid(
            PROJECT_INVALID.code,
            format!("`{raw}` is not a .xyz project entry point"),
        )
        .remedy(PROJECT_INVALID.remedy));
    }
    Ok(project)
}

fn has_extension(path: &str, extension: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, found)| found.eq_ignore_ascii_case(extension))
}

/// A folder the run will create: absent, with an existing parent, not on C:.
fn new_folder(raw: &str, flag: &str) -> Result<String, Failure> {
    let path = absolute(raw)?;
    let text = display(&path);
    if on_system_drive(&text) {
        return Err(Failure::invalid(
            SYSTEM_DRIVE_REFUSED.code,
            format!("--{flag} `{text}` is on C:"),
        )
        .remedy(SYSTEM_DRIVE_REFUSED.remedy));
    }
    if path.exists() {
        return Err(
            Failure::conflict(OUTPUT_EXISTS.code, format!("`{text}` already exists"))
                .remedy(OUTPUT_EXISTS.remedy),
        );
    }
    match path.parent() {
        Some(parent) if parent.is_dir() => Ok(text),
        _ => Err(Failure::invalid(
            OUTPUT_PARENT_MISSING.code,
            format!("the parent of --{flag} `{text}` does not exist"),
        )
        .remedy(OUTPUT_PARENT_MISSING.remedy)),
    }
}

/// The owner's rule, which `pls-deliver-autosag.ps1` and the backup driver
/// enforce with `^[Cc]:`: project work lives on the project Drive, never on
/// the system drive. Applied to every folder a verb creates, so one verb is
/// not stricter than the next.
fn on_system_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].eq_ignore_ascii_case(&b'c') && bytes[1] == b':'
}

/// `--sha256` as the drivers take it (64 lowercase hex), from `sha256:<hex>`
/// or the bare digits.
fn digest_argument(raw: &str) -> Result<String, Failure> {
    let hex = raw
        .strip_prefix("sha256:")
        .unwrap_or(raw)
        .to_ascii_lowercase();
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Failure::invalid(
            INVALID_DIGEST.code,
            format!("`{raw}` is not a sha256 digest"),
        )
        .remedy(INVALID_DIGEST.remedy));
    }
    Ok(hex)
}

/// The backup's digest: computed from its bytes, and compared with the pin
/// when there is one. The driver recomputes it before PLS-CADD opens the file.
fn backup_digest(backup: &str, pinned: Option<&str>) -> Result<String, Failure> {
    let actual = crate::file_digest(Path::new(backup))
        .and_then(|digest| digest.strip_prefix("sha256:").map(str::to_string))
        .ok_or_else(|| {
            Failure::invalid(
                SOURCE_NOT_FOUND.code,
                format!("`{backup}` could not be read"),
            )
            .remedy("check the path passed to --bak")
        })?;
    if let Some(pinned) = pinned {
        let expected = digest_argument(pinned)?;
        if expected != actual {
            return Err(Failure::conflict(
                BACKUP_DIGEST_MISMATCH.code,
                "the backup's bytes do not match --sha256",
            )
            .remedy(BACKUP_DIGEST_MISMATCH.remedy)
            .detail(json!({
                "expected": format!("sha256:{expected}"),
                "actual": format!("sha256:{actual}"),
            })));
        }
    }
    Ok(actual)
}

/// `--source-root` / `--project-file` pass through to the restore driver,
/// which validates them against the backup. Here only the shapes it can never
/// accept are refused: an empty value, or a leaf that is a path.
fn optional_text(raw: Option<&str>, flag: &str, leaf: bool) -> Result<Option<String>, Failure> {
    let Some(raw) = raw else { return Ok(None) };
    if raw.trim().is_empty() || raw.starts_with('-') || (leaf && raw.contains(['/', '\\'])) {
        return Err(Failure::invalid(
            INVALID_ARGUMENT.code,
            format!("--{flag} `{raw}` is not accepted"),
        )
        .remedy(if leaf {
            "pass the project's file name alone, e.g. nyamagabe.xyz"
        } else {
            r"pass one absolute Windows folder, e.g. C:\nyamagabe"
        }));
    }
    Ok(Some(raw.to_string()))
}

/// `--report-timeout`, bounded to what one report can reasonably take.
fn report_timeout(raw: Option<&str>) -> Result<String, Failure> {
    let seconds =
        ds_cli_contract::args::integer(raw.unwrap_or("1800"), "report-timeout", 60, 14_400)?;
    Ok(seconds.to_string())
}

/// Read a receipt a driver wrote, tolerating the UTF-8 BOM Windows
/// PowerShell 5.1 puts on some files.
fn read_receipt(path: &str) -> Result<Value, Failure> {
    let bytes = std::fs::read(path).map_err(|error| {
        Failure::failed(
            RESULT_UNREADABLE.code,
            format!("the receipt {path} is unreadable"),
        )
        .remedy(RESULT_UNREADABLE.remedy)
        .detail(json!({ "detail": error.kind().to_string() }))
    })?;
    parse_document(&bytes).ok_or_else(|| {
        Failure::failed(
            RESULT_UNREADABLE.code,
            format!("the receipt {path} is not JSON"),
        )
        .remedy(RESULT_UNREADABLE.remedy)
    })
}

fn parse_document(bytes: &[u8]) -> Option<Value> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    serde_json::from_slice(bytes).ok()
}

/// The fields every run verb's result carries: where its receipt is, and the
/// exact driver bundle that produced it.
fn provenance(receipt: &str) -> Value {
    json!({ "receipt": receipt, "drivers": bundle::digest() })
}
