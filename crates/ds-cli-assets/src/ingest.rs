//! `ds assets ingest` — one local file into the project's assets, explicitly.
//!
//! Ingest is the only way bytes enter this surface, and it is deliberately
//! narrow: one named file, confirmed with `--yes`, never a directory, never a
//! drag onto the map, never implied by a preview (§12.2, D13). The bytes go
//! through the project's existing resumable uploader — there is no second
//! uploader, digest or catalogue here (§12.10).
//!
//! `--sensitivity` is never inferred. Absent means the folder's default, and
//! failing that `internal` (D4); `open` is a class a person states, and one
//! only ds-brain can grant.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, FOLDER_ARG};

const PATH_ARG: Arg = Arg::value(
    "path",
    "<file>",
    "Absolute path of the existing local file to ingest.",
)
.required();

const SENSITIVITY_ARG: Arg = Arg::value(
    "sensitivity",
    "<class>",
    "The access class; absent means the folder's default, else internal. Never inferred as open.",
)
.choices(crate::SENSITIVITIES);

pub static COMMAND: Command = Command {
    id: "assets.ingest",
    path: &["assets", "ingest"],
    contract: 1,
    summary: "Add one local file to the project's assets, explicitly.",
    purpose: "\
Recognises the file's format from its head in the kernel, sends the bytes \
through the project's existing resumable uploader, and finalises the catalogue \
row in ds-brain. Ingest is never implicit: it needs --yes, dragging a file onto \
the map never sends anything, and a sensitivity is never inferred as open — the \
default is the folder's default or internal, and only a stricter class may be \
named here. Refused by name when offline in this slice.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[PATH_ARG, FOLDER_ARG, SENSITIVITY_ARG, DESCRIPTOR_ARG],
    output: "\
`asset` — the created row, with its `asset_id`, `digest`, `kind`, `format`, \
`folder` and `sensitivity`.",
    examples: &[Example {
        command: "ds assets ingest --path /home/me/Documents/EPC-Lot3-signed.pdf --folder contracts/2026/epc --sensitivity confidential --yes",
        note: "The row's asset_id feeds classify, attach and read from here on.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::ASSETS_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_SOURCE_PATH,
        crate::INVALID_FOLDER_PATH,
        crate::CONFIRMATION_REQUIRED,
        crate::ASSET_NOT_FOUND,
        crate::ASSET_CLASS_FORBIDDEN,
        crate::ASSET_VERSION_CONFLICT,
        crate::ASSET_REQUEST_INVALID,
        crate::ASSET_RULE_REFUSED,
        crate::ASSETS_NOT_IMPLEMENTED,
        crate::ASSETS_SERVICE_FAILED,
        crate::OFFLINE,
        crate::BACKEND_UNREACHABLE,
        crate::ASSETS_OFFLINE_WRITE,
        crate::UNKNOWN_FOLDER,
    ],
    reference: Some("docs/reference/assets.md"),
    availability: crate::paired_availability,
};

/// The ingest, validated locally, in the exact keys the operation declares.
fn arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("path".into(), json!(source_file(inputs.require("path")?)?));
    if let Some(folder) = inputs.value("folder") {
        arguments.insert(
            "folder".into(),
            json!(crate::folder_path(folder, "folder")?),
        );
    }
    // The class is closed at the parser. An absent flag stays absent: the
    // folder's default, and failing that `internal`, is the answer to a
    // question nobody asked — and it is never `open` (D4).
    if let Some(sensitivity) = inputs.value("sensitivity") {
        arguments.insert("sensitivity".into(), json!(sensitivity));
    }
    Ok(Value::Object(arguments))
}

/// `--path`, held to an absolute path naming an existing regular file.
///
/// `ds` may read a local file; it never runs one, and it never resolves a
/// relative one. The process that will read these bytes is the desktop, with
/// its own working directory, so a path that resolves here would resolve
/// somewhere else — or nowhere — there. A directory is refused rather than
/// walked: ingest is one file, explicitly.
fn source_file(raw: &str) -> Result<String, Failure> {
    let refuse = |why: &str| {
        Failure::invalid("invalid_source_path", format!("`--path` {why}"))
            .remedy(crate::INVALID_SOURCE_PATH.remedy)
            .detail(json!({ "given": raw }))
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return Err(refuse("is empty, or holds a control character"));
    }
    let path = Path::new(trimmed);
    if !path.is_absolute() {
        return Err(refuse(
            "is not absolute, and the application resolves it in its own working directory",
        ));
    }
    let metadata = std::fs::metadata(path)
        .map_err(|_| refuse("does not name a readable file on this machine"))?;
    if !metadata.is_file() {
        return Err(refuse(
            "is not a regular file; ingest takes one file at a time",
        ));
    }
    Ok(trimmed.to_string())
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ASSETS_INGEST,
        arguments,
        crate::INGEST_TIMEOUT,
    )
    .map_err(crate::classify_assets_failure)
}

pub fn render(data: &Value) -> String {
    let row = &data["asset"];
    let mut out = format!("ingested {}\n", row["asset_id"].as_str().unwrap_or("?"));
    if row.is_object() {
        out.push_str(&crate::asset_line(row));
    }
    if let Some(digest) = row["digest"].as_str().filter(|digest| !digest.is_empty()) {
        out.push_str(&format!("  {digest}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::spec::ArgKind;
    use ds_cli_desktop::ops::undeclared_key;

    /// A file that certainly exists on any machine building this crate, and
    /// the directory that holds it.
    const CRATE_DIR: &str = env!("CARGO_MANIFEST_DIR");

    fn manifest() -> String {
        format!("{CRATE_DIR}/Cargo.toml")
    }

    fn parse(tokens: &[&str]) -> Inputs {
        let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
    }

    #[test]
    fn the_source_is_an_absolute_path_to_a_file_that_exists_here() {
        assert_eq!(source_file(&manifest()).expect("valid"), manifest());
        for bad in ["", "   ", "Cargo.toml", "./Cargo.toml", CRATE_DIR] {
            assert_eq!(
                source_file(bad).expect_err("must refuse").code(),
                "invalid_source_path",
                "`{bad}` was accepted as a source file"
            );
        }
        let missing = format!("{CRATE_DIR}/no-such-file-9f3c.bin");
        let failure = source_file(&missing).expect_err("must refuse");
        assert_eq!(failure.code(), "invalid_source_path");
        assert!(failure.message().contains("readable file"));
    }

    #[test]
    fn a_directory_is_refused_rather_than_walked() {
        let failure = source_file(CRATE_DIR).expect_err("must refuse");
        assert!(failure.message().contains("one file at a time"));
    }

    #[test]
    fn an_explicit_class_travels_and_an_absent_one_is_never_inferred() {
        // D4: the default is the folder's, else internal — decided by the
        // owner of the catalogue, never guessed at here. `open` is a word a
        // person says; ds-brain still decides whether they may.
        let inferred = arguments(&parse(&["--path", &manifest()])).expect("valid");
        assert!(
            inferred.get("sensitivity").is_none(),
            "an absent class must not be invented"
        );
        let explicit =
            arguments(&parse(&["--path", &manifest(), "--sensitivity", "open"])).expect("valid");
        assert_eq!(explicit["sensitivity"], json!("open"));
    }

    #[test]
    fn the_payload_carries_exactly_the_keys_the_operation_declares() {
        let payload = arguments(&parse(&[
            "--path",
            &manifest(),
            "--folder",
            " contracts/2026/epc ",
            "--sensitivity",
            "confidential",
        ]))
        .expect("valid");
        assert_eq!(undeclared_key(&crate::ASSETS_INGEST, &payload), None);
        let mut keys: Vec<&str> = payload
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["folder", "path", "sensitivity"]);
        assert_eq!(payload["folder"], json!("contracts/2026/epc"));
        assert_eq!(
            arguments(&parse(&["--path", &manifest(), "--folder", "/contracts"]))
                .expect_err("must refuse")
                .code(),
            "invalid_folder_path"
        );
    }

    #[test]
    fn the_human_projection_reports_the_row_and_its_digest() {
        let rendered = render(&json!({
            "asset": { "asset_id": "a_7kq3nr2v0b1c", "name": "EPC-Lot3-signed.pdf",
                       "folder": "contracts/2026/epc", "kind": "doc", "format": "pdf",
                       "status": "fresh", "sensitivity": "confidential", "bytes": 812_004,
                       "digest": "sha256:abc" }
        }));
        assert!(rendered.contains("ingested a_7kq3nr2v0b1c"));
        assert!(rendered.contains("contracts/2026/epc/EPC-Lot3-signed.pdf"));
        assert!(rendered.contains("sha256:abc"));
    }

    #[test]
    fn this_write_cannot_be_reached_without_explicit_confirmation() {
        // The gate itself lives once, in `ds`'s dispatch, and reads exactly
        // this declaration — so the declaration is the part a test inside
        // this crate can hold. A confirmation-gated command may also take no
        // positional argument, because `--yes` must never be the thing that
        // shifts what an operand means.
        assert!(COMMAND.effect.needs_confirmation());
        assert!(COMMAND.confirmation_required_for(&parse(&["--path", &manifest()])));
        assert!(
            COMMAND
                .args
                .iter()
                .all(|arg| arg.kind != ArgKind::Positional)
        );
    }
}
