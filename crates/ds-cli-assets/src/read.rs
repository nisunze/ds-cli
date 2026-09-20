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

use crate::{ASSET_ARG, LANE_ARG, MEMBER_ARG};

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
Fetches the asset's bytes through the catalogue's signed read, verifies them \
against the row's digest, and writes them to --out on the host running `ds`: \
a new absolute path only, written to a temporary sibling and renamed. An \
existing file is never overwritten. With --member, one member of a pack is \
extracted by the kernel and written instead. A restricted or confidential \
asset the caller may not read is refused by class or reported absent, exactly \
as the listing does. Reads up to 32 MiB; a projected sys: row is refused by \
name. Headless: no window.",
    chapter: Chapter::Assets,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[ASSET_ARG, MEMBER_ARG, OUT_ARG, LANE_ARG],
    output: "\
The `path` written, its `bytes` and `digest` (sha256), and the `asset_id` and \
`member` it came from.",
    examples: &[Example {
        command: "ds assets read --asset a_7kq3nr2v0b1c --out /home/me/Downloads/EPC-Lot3-signed.pdf --output json",
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
    let asset_id = arguments["asset"].as_str().unwrap_or_default().to_owned();
    let member = arguments["member"].as_str().map(str::to_owned);
    let out = arguments["out"].as_str().unwrap_or_default().to_owned();

    let (row, bytes) = crate::bytes(lane, &asset_id)?;
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
mod tests {
    use ds_cli_contract::args::parse;
    use ds_cli_contract::output::{Format, Output};

    use super::*;

    fn context() -> Context {
        Context {
            confirmed: false,
            output: Output::resolve(Format::Json, false, true),
        }
    }

    /// A destination that does not exist, under a directory that does.
    fn new_file(label: &str) -> String {
        std::env::temp_dir()
            .join(format!(
                "ds-cli-assets-read-{}-{label}.bin",
                std::process::id()
            ))
            .display()
            .to_string()
    }

    /// Every local refusal below is proved on the arguments alone: nothing
    /// is fetched and no file is written by this test module.
    fn unpaired() -> [String; 0] {
        []
    }

    fn refusal(flags: &[&str]) -> String {
        let tokens: Vec<String> = flags.iter().map(|flag| (*flag).to_string()).collect();
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        arguments(&inputs)
            .expect_err("a malformed read is refused before any round trip")
            .code()
            .to_string()
    }

    #[test]
    fn the_asset_and_the_destination_are_refused_by_name_before_any_round_trip() {
        assert_eq!(
            refusal(&["--asset", "a_7Kq3nR2v", "--out", &new_file("shape")]),
            "invalid_asset_id"
        );
        for bad in [
            "",
            "  ",
            "Downloads/lot3.pdf",
            "./lot3.pdf",
            "~/lot3.pdf",
            "/home/me/Downloads/",
            "/home/me/../me/lot3.pdf",
        ] {
            assert_eq!(
                refusal(&["--asset", "a_7kq3nr2v0b1c", "--out", bad]),
                "invalid_out_path",
                "`{bad}` was accepted as a destination"
            );
        }
    }

    #[test]
    fn an_existing_destination_or_a_missing_directory_is_refused_here_not_after_a_round_trip() {
        // The desktop shares this filesystem, so what exists here exists
        // there. An existing file — and an existing directory named as the
        // file, and a dangling link — is refused by name; so is a directory
        // that is not there to write into. Both would otherwise cost a
        // project round trip, and possibly a project switch, to learn.
        let existing = new_file("existing");
        std::fs::write(&existing, b"already here").expect("temp file is writable");
        let failure = {
            let mut tokens = ["--asset", "a_7kq3nr2v0b1c", "--out", existing.as_str()]
                .map(str::to_string)
                .to_vec();
            tokens.extend(unpaired());
            let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
            arguments(&inputs).expect_err("an existing file is never overwritten")
        };
        let _ = std::fs::remove_file(&existing);
        assert_eq!(failure.code(), "invalid_out_path");
        assert!(
            failure.message().contains("already exists"),
            "the refusal must say the file exists: {}",
            failure.message()
        );

        let directory = std::env::temp_dir().display().to_string();
        assert_eq!(
            refusal(&["--asset", "a_7kq3nr2v0b1c", "--out", &directory]),
            "invalid_out_path",
            "an existing directory named as the file was accepted"
        );

        let missing_parent = std::env::temp_dir()
            .join(format!(
                "ds-cli-assets-read-{}-no-such-dir",
                std::process::id()
            ))
            .join("lot3.pdf")
            .display()
            .to_string();
        assert_eq!(
            refusal(&["--asset", "a_7kq3nr2v0b1c", "--out", &missing_parent]),
            "invalid_out_path",
            "a destination under a missing directory was accepted"
        );
    }

    #[test]
    fn a_well_formed_read_passes_every_local_check() {
        // A member may be named; the only thing left is the catalogue, which
        // a unit test does not reach. A projected `sys:` id passes the local
        // checks too and is refused by name only when its bytes are asked for.
        let out = new_file("well-formed");
        let tokens: Vec<String> = [
            "--asset",
            "sys:design_attachment:att_1:rev_2",
            "--member",
            "Lot3/gis/poles.shp",
            "--out",
            &out,
        ]
        .map(str::to_string)
        .to_vec();
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        assert!(arguments(&inputs).is_ok());
        let _ = context();
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        // The handler is held against its own declaration, with every flag
        // set.
        let out = new_file("payload");
        let mut tokens = [
            "--asset",
            "a_7kq3nr2v0b1c",
            "--member",
            " Lot3/gis/poles.shp ",
            "--out",
            out.as_str(),
        ]
        .map(str::to_string)
        .to_vec();
        tokens.extend(unpaired());
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        let payload = arguments(&inputs).expect("valid");
        let mut keys: Vec<&str> = payload
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["asset", "member", "out"]);
        assert_eq!(payload["member"], json!("Lot3/gis/poles.shp"));

        // A whole-asset read sends no member at all rather than an empty one.
        let mut tokens = [
            "--asset",
            "a_7kq3nr2v0b1c",
            "--member",
            "  ",
            "--out",
            out.as_str(),
        ]
        .map(str::to_string)
        .to_vec();
        tokens.extend(unpaired());
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        let whole = arguments(&inputs).expect("valid");
        assert!(whole.get("member").is_none());
    }

    #[test]
    fn the_receipt_renders_the_path_bytes_and_digest() {
        let out = render(&json!({
            "path": "/home/me/Downloads/EPC-Lot3-signed.pdf",
            "bytes": 811_233,
            "digest": "b1946ac92492d2347c6235b4d2611184",
            "asset_id": "a_7kq3nr2v0b1c",
            "member": null,
        }));
        assert_eq!(
            out,
            "wrote /home/me/Downloads/EPC-Lot3-signed.pdf\n  \
             811233 bytes · sha256 b1946ac92492d2347c6235b4d2611184\n  \
             from a_7kq3nr2v0b1c\n"
        );
        let member = render(&json!({
            "path": "/home/me/Downloads/poles.shp",
            "bytes": 1,
            "digest": "d1",
            "asset_id": "a_7kq3nr2v0b1c",
            "member": "Lot3/gis/poles.shp",
        }));
        assert!(member.contains("1 byte · sha256 d1"));
        assert!(member.ends_with("from a_7kq3nr2v0b1c · Lot3/gis/poles.shp\n"));
    }
}
