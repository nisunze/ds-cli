//! `ds assets read` — one asset's bytes, or one pack member's, to a new file.
//!
//! The bytes never cross the bridge (decision D22). This command names the
//! asset and the destination; the paired desktop fetches the source's own
//! signed read, writes a temporary sibling, verifies the digest and renames
//! it. What comes back is the receipt for a file that already exists on disk.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{ASSET_ARG, DESCRIPTOR_ARG, MEMBER_ARG};

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
Fetches the asset's bytes through its source's own signed read and has the \
paired desktop write them to --out through one closed native command: a new \
absolute path only, written to a temporary sibling, digest-verified and \
renamed. Bytes never cross the bridge and an existing file is never \
overwritten. A restricted or confidential asset the caller may not read is \
refused by class or reported absent, exactly as the listing does. Offline, a \
cached copy is written; otherwise the refusal says the bytes are not held.",
    chapter: Chapter::Assets,
    effect: Effect::LocalFileWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[ASSET_ARG, MEMBER_ARG, OUT_ARG, DESCRIPTOR_ARG],
    output: "\
The `path` written, its `bytes` and `digest` (sha256), and the `asset_id` and \
`member` it came from.",
    examples: &[Example {
        command: "ds assets read --asset a_7kq3nr2v0b1c --out /home/me/Downloads/EPC-Lot3-signed.pdf --output json",
        note: "Compare .data.digest with the catalogue row's before trusting the file.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::PROJECT_NOT_OPEN,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::ASSETS_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_ASSET_ID,
        crate::INVALID_OUT_PATH,
        crate::ASSET_NOT_FOUND,
        crate::ASSET_CLASS_FORBIDDEN,
        crate::ASSET_REQUEST_INVALID,
        crate::ASSET_RULE_REFUSED,
        crate::ASSETS_NOT_IMPLEMENTED,
        crate::ASSETS_SERVICE_FAILED,
        crate::OFFLINE,
        crate::BACKEND_UNREACHABLE,
        crate::ASSET_IS_NOT_A_FILE,
        crate::ASSET_TOO_LARGE,
        crate::ORIGIN_READ_FAILED,
        crate::ORIGIN_UNREACHABLE,
        crate::ORIGIN_READ_UNAVAILABLE,
        crate::INVALID_MEMBER,
    ],
    reference: Some("docs/reference/assets.md"),
    availability: crate::paired_availability,
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
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ASSETS_READ,
        arguments,
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_assets_failure)
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
    use ds_cli_desktop::ops::undeclared_key;

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

    /// A descriptor path that cannot exist, so nothing pairs and no file is
    /// ever written by this test module.
    fn unpaired() -> [String; 2] {
        [
            "--desktop-descriptor".to_string(),
            std::env::temp_dir()
                .join(format!(
                    "ds-cli-assets-read-{}-absent.json",
                    std::process::id()
                ))
                .display()
                .to_string(),
        ]
    }

    fn refusal(flags: &[&str]) -> String {
        let mut tokens: Vec<String> = flags.iter().map(|flag| (*flag).to_string()).collect();
        tokens.extend(unpaired());
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        run(&inputs, &context())
            .expect_err("an unpaired read cannot write a file")
            .code()
            .to_string()
    }

    #[test]
    fn the_asset_and_the_destination_are_refused_by_name_before_the_bridge() {
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
            run(&inputs, &context()).expect_err("an existing file is never overwritten")
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
    fn a_well_formed_read_reaches_the_pairing_boundary() {
        // A projected `sys:` asset may be read (§7.1), and a member may be
        // named; the only thing left to refuse is the absent desktop.
        let out = new_file("well-formed");
        let code = refusal(&[
            "--asset",
            "sys:design_attachment:att_1:rev_2",
            "--member",
            "Lot3/gis/poles.shp",
            "--out",
            &out,
        ]);
        assert!(
            !code.starts_with("invalid_"),
            "a well-formed read was refused locally as `{code}`"
        );
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        // `invoke` refuses an undeclared key, but only once a desktop has
        // paired — which no CI machine has. This is the one place the
        // handler is held against its own declaration.
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
        assert_eq!(undeclared_key(&crate::ASSETS_READ, &payload), None);
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
