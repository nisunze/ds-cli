//! `ds pls desktop toolkit` — the whole embedded toolkit, written for an
//! install that wants the files themselves.
//!
//! The adapters are product code owned by this crate, and another installer,
//! a reviewer or an operator driving PLS-CADD by hand profits from them only
//! if they can obtain the exact bytes. This verb writes every embedded file
//! into one new folder with the bundle's layout, reads each back against its
//! pin, and adds a manifest naming every path and SHA-256. It only writes
//! files, so it answers on Linux and Windows alike, and it starts no process.
//!
//! The copy is the operator's. No `ds` verb ever reads it back: the run verbs
//! re-materialize their own verified bytes per run.

use std::io::Write as _;
use std::path::Path;

use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

use super::bundle::{self, AREAS, BUNDLE};
use super::{ADAPTERS_NOT_EMBEDDED, INVALID_ARGUMENT, OUTPUT_EXISTS, OUTPUT_PARENT_MISSING};

/// The manifest's name at the toolkit root. No embedded path may take it.
pub const MANIFEST: &str = "toolkit-manifest.json";
pub const MANIFEST_SCHEMA: &str = "ds.pls.desktop_toolkit.v1";

pub const TOOLKIT_WRITE_FAILED: Refusal = Refusal {
    code: "toolkit_write_failed",
    when: "a file could not be written, synchronized or read back against its pin",
    remedy: "check the folder is writable and has space, then retry into a new folder",
};

pub static COMMAND: Command = Command {
    id: "pls.desktop.toolkit",
    path: &["pls", "desktop", "toolkit"],
    contract: 1,
    summary: "Write the whole PLS-CADD desktop toolkit, digest-pinned, to a folder.",
    purpose: "Gives any install the complete product-owned toolkit this ds embeds: the third-party adapters that drive PLS-CADD and PLS-POLE 16.81 and Microsoft Word, the ds entry scripts, the dialog catalogue and native profiles, their PowerShell tests, and the lab tools still awaiting a Rust owner, with toolkit-manifest.json naming every file's SHA-256. Use it to review the adapters, run one by hand, or install them on another machine. It only writes files and starts nothing, so it works on Linux and Windows. The desktop verbs never read this copy: each run re-materializes its own verified bytes.",
    chapter: Chapter::PlsCadd,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "out",
        "<new folder>",
        "Absent folder to create; its parent must exist.",
    )
    .required()],
    output: "The folder, its manifest path, the toolkit digest the desktop receipts carry, the file count and bytes, and the count per area (adapters/pls-cadd, adapters/word, lab). Every path and SHA-256 is in the manifest.",
    examples: &[
        Example {
            command: "ds pls desktop toolkit --out ./pls-toolkit",
            note: "Every embedded file plus toolkit-manifest.json.",
            runnable: false,
        },
        Example {
            command: r"ds pls desktop toolkit --out G:\Tools\pls-toolkit-2026-10-02 --output json",
            note: "A second run into the same folder refuses output_exists.",
            runnable: false,
        },
    ],
    refusals: &[
        ADAPTERS_NOT_EMBEDDED,
        INVALID_ARGUMENT,
        OUTPUT_EXISTS,
        OUTPUT_PARENT_MISSING,
        TOOLKIT_WRITE_FAILED,
    ],
    reference: Some("docs/reference/pls.md"),
    search: &["pls toolkit", "powershell drivers", "pls adapters"],
    requires: Requires::Server,
    availability: super::adapter_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    if !bundle::embedded() {
        return Err(Failure::unavailable(
            ADAPTERS_NOT_EMBEDDED.code,
            "this ds was built without the third-party adapter layer",
        )
        .remedy(ADAPTERS_NOT_EMBEDDED.remedy));
    }
    let root = super::absolute(inputs.require("out")?)?;
    write(&root)
}

/// Create `root`, write the bundle and its manifest into it, and describe
/// the result. The folder is created, never reused; on any failure the
/// folder this call created is removed again.
pub fn write(root: &Path) -> Result<Value, Failure> {
    let shown = super::display(root);
    match root.parent() {
        Some(parent) if parent.is_dir() => {}
        _ => {
            return Err(Failure::invalid(
                OUTPUT_PARENT_MISSING.code,
                format!("the parent of --out `{shown}` does not exist"),
            )
            .remedy(OUTPUT_PARENT_MISSING.remedy));
        }
    }
    if let Err(error) = std::fs::create_dir(root) {
        return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
            Failure::conflict(OUTPUT_EXISTS.code, format!("`{shown}` already exists"))
                .remedy(OUTPUT_EXISTS.remedy)
        } else {
            write_failure(&shown, &error)
        });
    }
    let written = bundle::materialize(root)
        .map_err(|unwritten| {
            Failure::failed(TOOLKIT_WRITE_FAILED.code, unwritten.message())
                .remedy(TOOLKIT_WRITE_FAILED.remedy)
                .detail(unwritten.detail())
        })
        .and_then(|()| write_manifest(root));
    if let Err(failure) = written {
        let _ = std::fs::remove_dir_all(root);
        return Err(failure);
    }
    let mut areas = serde_json::Map::new();
    for area in AREAS {
        let count = BUNDLE.iter().filter(|s| s.path.starts_with(area)).count();
        areas.insert(area.trim_end_matches('/').to_string(), json!(count));
    }
    Ok(json!({
        "out": shown,
        "manifest": super::display(&root.join(MANIFEST)),
        "toolkit": bundle::digest(),
        "files": BUNDLE.len(),
        "bytes": BUNDLE.iter().map(|s| s.bytes.len()).sum::<usize>(),
        "areas": areas,
    }))
}

/// The manifest an install checks its copy against: every path in table
/// order with the same lowercase hex pin `bundle.rs` holds.
pub fn manifest() -> Value {
    json!({
        "schema": MANIFEST_SCHEMA,
        "ds_version": env!("CARGO_PKG_VERSION"),
        "toolkit": bundle::digest(),
        "files": BUNDLE
            .iter()
            .map(|s| json!({ "path": s.path, "sha256": s.sha256, "bytes": s.bytes.len() }))
            .collect::<Vec<_>>(),
    })
}

fn write_manifest(root: &Path) -> Result<(), Failure> {
    let path = root.join(MANIFEST);
    let shown = super::display(&path);
    let mut bytes = serde_json::to_vec_pretty(&manifest())
        .map_err(|error| write_failure(&shown, &std::io::Error::other(error)))?;
    bytes.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| write_failure(&shown, &error))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| write_failure(&shown, &error))
}

fn write_failure(path: &str, error: &std::io::Error) -> Failure {
    Failure::failed(TOOLKIT_WRITE_FAILED.code, format!("could not write {path}"))
        .remedy(TOOLKIT_WRITE_FAILED.remedy)
        .detail(json!({ "detail": error.kind().to_string() }))
}

pub fn render(data: &Value) -> String {
    let mut text = format!(
        "PLS-CADD desktop toolkit written to {}\n  {} files, {} bytes, toolkit {}\n",
        data["out"].as_str().unwrap_or(""),
        data["files"],
        data["bytes"],
        data["toolkit"].as_str().unwrap_or(""),
    );
    if let Some(areas) = data["areas"].as_object() {
        for (area, count) in areas {
            text.push_str(&format!("  {area:<18} {count}\n"));
        }
    }
    text.push_str(&format!(
        "  manifest {}\n",
        data["manifest"].as_str().unwrap_or("")
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let parent = std::env::temp_dir().join(format!(
            "ds-pls-toolkit-{name}-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&parent).unwrap();
        parent
    }

    #[cfg(feature = "desktop-adapters")]
    #[test]
    fn writes_every_file_and_a_manifest_that_matches_the_pins_then_refuses_a_rerun() {
        use sha2::{Digest, Sha256};
        let parent = scratch("write");
        let root = parent.join("toolkit");
        let answer = write(&root).expect("the toolkit writes");
        assert_eq!(answer["files"], BUNDLE.len());
        assert_eq!(
            answer["areas"]["adapters/word"], 2,
            "the Word adapter and its README"
        );
        assert!(answer["areas"]["adapters/pls-cadd"].as_u64().unwrap() > 50);
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(root.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(manifest["schema"], MANIFEST_SCHEMA);
        assert_eq!(manifest["toolkit"], bundle::digest());
        let files = manifest["files"].as_array().unwrap();
        assert_eq!(files.len(), BUNDLE.len());
        for (entry, script) in files.iter().zip(BUNDLE) {
            assert_eq!(entry["path"], script.path);
            assert_eq!(entry["sha256"], script.sha256);
            let on_disk = std::fs::read(bundle::below(&root, script.path)).unwrap();
            assert_eq!(format!("{:x}", Sha256::digest(&on_disk)), script.sha256);
        }
        assert!(BUNDLE.iter().all(|s| s.path != MANIFEST));

        let again = write(&root).unwrap_err();
        assert_eq!(again.code(), OUTPUT_EXISTS.code);
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn refuses_a_missing_parent_and_writes_nothing() {
        let parent = scratch("parent");
        let root = parent.join("absent").join("toolkit");
        let refusal = write(&root).unwrap_err();
        assert_eq!(refusal.code(), OUTPUT_PARENT_MISSING.code);
        assert!(!parent.join("absent").exists());
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[cfg(not(feature = "desktop-adapters"))]
    #[test]
    fn without_the_adapter_layer_the_run_refuses_and_writes_nothing() {
        let parent = scratch("off");
        let root = parent.join("toolkit");
        let inputs =
            ds_cli_contract::parse(&COMMAND, &["--out".to_string(), root.display().to_string()])
                .expect("the arguments parse");
        let context = Context {
            confirmed: false,
            output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
        };
        let refusal = run(&inputs, &context).unwrap_err();
        assert_eq!(refusal.code(), ADAPTERS_NOT_EMBEDDED.code);
        assert!(!root.exists());
        std::fs::remove_dir_all(&parent).unwrap();
    }
}
