//! `ds assets read` — one asset's bytes, or one pack member's, to a new file.
//!
//! This command names the asset and the destination; the native client
//! fetches the catalogue's signed read, verifies the bytes against the row's
//! digest, and this host writes a temporary sibling and renames it. What
//! comes back is the receipt for a file that already exists on disk.

use std::io::Write;
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{ASSET_ARG, LANE_ARG, MEMBER_ARG, PROJECT_ARG};

const OUT_ARG: Arg = Arg::value(
    "out",
    "<file>",
    "New absolute destination file; never overwritten.",
)
.required();

/// A destination longer than this is a mistake, not a path: every platform
/// this runs on refuses it far below the limit.
const MAX_OUT_CHARS: usize = 4_096;

pub static COMMAND: Command = Command {
    id: "assets.read",
    path: &["assets", "read"],
    contract: 1,
    summary: "Save one asset's bytes, or one pack member, to a new file.",
    purpose: "\
Fetches the asset's bytes through its signed read, verifies them against the \
row's digest, and writes them to --out: a new absolute path, written via a \
temporary sibling, never over an existing file. With --member, one pack \
member is extracted by the kernel instead. Asset classes are enforced as in \
the listing. Reads up to 32 MiB, including Project Work attachments \
(sys:pm_attachment:<id>); other sys: rows are refused by name.",
    chapter: Chapter::Assets,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[ASSET_ARG, MEMBER_ARG, OUT_ARG, LANE_ARG, PROJECT_ARG],
    output: "\
The `path` written, its `bytes` and `digest` (sha256), and the `asset_id` and \
`member` it came from.",
    examples: &[Example {
        command: "ds assets read --project <exact-id> --asset a_7kq3nr2v0b1c --out /home/me/Downloads/EPC-Lot3-signed.pdf --output json",
        note: "Compare .data.digest with the catalogue row's before trusting the file.",
        runnable: false,
    }],
    refusals: &crate::refusals::<29>(&[
        crate::INVALID_ASSET_ID,
        crate::INVALID_MEMBER,
        crate::INVALID_OUT_PATH,
        crate::ASSET_TOO_LARGE,
        crate::ORIGIN_READ_FAILED,
        crate::ORIGIN_READ_UNAVAILABLE,
        crate::ASSETS_UNREADABLE,
    ]),
    reference: Some("docs/reference/assets.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The read, validated locally, in the exact keys the operation declares.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert(
        "asset".into(),
        json!(crate::asset_id(inputs.require("asset")?, "asset")?),
    );
    // A whole-asset read sends no member at all rather than an empty one,
    // which the walk would read as a member named "".
    if let Some(member) = inputs
        .value("member")
        .map(str::trim)
        .filter(|member| !member.is_empty())
    {
        arguments.insert("member".into(), json!(member));
    }
    arguments.insert("out".into(), json!(destination(inputs.require("out")?)?));
    Ok(Value::Object(arguments))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    let asset_id = arguments["asset"].as_str().unwrap_or_default().to_owned();
    let member = arguments["member"].as_str().map(str::to_owned);
    let out = arguments["out"].as_str().unwrap_or_default().to_owned();

    let (row, bytes) = crate::bytes(lane, project, &asset_id)?;
    let (payload, digest) = match member.as_deref() {
        None => {
            let digest = row["digest"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| crate::digest_of(&bytes));
            (bytes, digest)
        }
        Some(member) => {
            let extracted = crate::with_bytes(
                &bytes,
                &json!({
                    "schema": crate::REQUEST_SCHEMA,
                    "action": "extract_member",
                    "member": member,
                }),
            )?;
            let digest = extracted["digest"].as_str().unwrap_or_default().to_owned();
            (crate::decode_base64(&extracted["bytes_b64"])?, digest)
        }
    };
    write_new_file(&out, &payload)?;
    Ok(json!({
        "path": out,
        "bytes": payload.len(),
        "digest": digest,
        "asset_id": asset_id,
        "member": member,
    }))
}

/// Write to a temporary sibling and rename, so a half-written file never
/// carries the destination's name; the destination was checked to be new
/// before any byte was fetched, and is checked again here.
fn write_new_file(out: &str, bytes: &[u8]) -> Result<(), Failure> {
    let failed = |why: String| {
        Failure::failed("origin_read_failed", why).remedy(crate::ORIGIN_READ_FAILED.remedy)
    };
    let path = Path::new(out);
    let parent = path
        .parent()
        .ok_or_else(|| failed("the destination has no parent directory".into()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| failed("the destination has no file name".into()))?;
    let temporary = parent.join(format!(".{name}.ds-{}.part", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| failed(format!("could not create {}: {error}", temporary.display())))?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| failed(format!("could not write {}: {error}", temporary.display())));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    if std::fs::symlink_metadata(path).is_ok() {
        let _ = std::fs::remove_file(&temporary);
        return Err(Failure::invalid(
            "invalid_out_path",
            "`--out` came into existence while the bytes were fetched; assets are never written over an existing file",
        )
        .remedy(crate::INVALID_OUT_PATH.remedy));
    }
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        failed(format!("could not rename into {out}: {error}"))
    })
}

/// The destination: a path, absolute, a file rather than a directory, free
/// of traversal, not yet existing, and under a directory that does.
///
/// The desktop and this process share one filesystem — the bridge is
/// loopback — so what is on disk here is what the desktop will find. A
/// destination that already exists, or whose directory does not, is refused
/// by name before a project round trip, and before any project switch that
/// round trip would route. The *guarantee* stays the desktop's: its one closed
/// native command refuses an existing path atomically at the moment of the
/// write (D22), so a file that appears between this check and that write is
/// still never overwritten — that refusal crosses the bridge with its own
/// message intact.
fn destination(raw: &str) -> Result<String, Failure> {
    let refuse = |why: &str| {
        Failure::invalid("invalid_out_path", format!("`--out` {why}"))
            .remedy(crate::INVALID_OUT_PATH.remedy)
            .detail(json!({ "given": raw }))
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(refuse("is empty"));
    }
    if trimmed.chars().count() > MAX_OUT_CHARS {
        return Err(refuse("is longer than 4,096 characters"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(refuse("holds a control character"));
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(refuse(
            "must be an absolute path, so it cannot depend on where `ds` was run",
        ));
    }
    if trimmed.ends_with('/') || trimmed.ends_with('\\') {
        return Err(refuse("names a directory; name the file to create"));
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(refuse(
            "holds a `..` segment; name the destination directly",
        ));
    }
    if path.file_name().is_none() {
        return Err(refuse("has no file name"));
    }
    // `symlink_metadata` so a dangling link counts as existing: the desktop
    // would refuse to write through it, and "already exists" is the truth.
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(refuse(
            "already exists; assets are never written over an existing file, so choose a new name",
        ));
    }
    match path.parent() {
        Some(parent) if parent.is_dir() => {}
        _ => {
            return Err(refuse(
                "is under a directory that does not exist; create the directory first",
            ));
        }
    }
    Ok(trimmed.to_string())
}

pub fn render(data: &Value) -> String {
    let path = data["path"]
        .as_str()
        .or_else(|| data["out"].as_str())
        .unwrap_or("?");
    let mut out = format!("wrote {path}\n");
    out.push_str(&format!(
        "  {} · sha256 {}\n",
        crate::plural(data["bytes"].as_u64().unwrap_or(0), "byte"),
        data["digest"].as_str().unwrap_or("—"),
    ));
    let mut source = data["asset_id"].as_str().unwrap_or("?").to_string();
    if let Some(member) = data["member"].as_str().filter(|member| !member.is_empty()) {
        source.push_str(&format!(" · {}", crate::truncate(member, 60)));
    }
    out.push_str(&format!("  from {source}\n"));
    out
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
