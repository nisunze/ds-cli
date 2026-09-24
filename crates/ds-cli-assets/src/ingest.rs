//! `ds assets ingest` — one local file into the project's assets, explicitly.
//!
//! Ingest is the only way bytes enter this surface, and it is deliberately
//! narrow: one named file, confirmed with `--yes`, never a directory, never a
//! drag onto the map, never implied by a preview (§12.2, D13). The bytes go
//! through the project's existing resumable uploader — there is no second
//! uploader, digest or catalogue here (§12.10). The boundary is three steps
//! the native client walks as one: `ingest_start` mints the upload target,
//! the file streams to it from the path named here, `ingest_finalize` seals
//! the row against the digest this command computed.
//!
//! `--sensitivity` is never inferred. Absent means the folder's default, and
//! failing that `internal` (D4); `open` is a class a person states, and one
//! only ds-brain can grant.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_assets::{IngestRequest, MAX_INGEST_BYTES, RECOGNISE_HEAD_BYTES};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{CatalogueCommand, FOLDER_ARG, LANE_ARG, PROJECT_ARG};

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
Recognises the file's format from its head in the kernel, digests the whole \
file in one streaming pass, mints the upload target in ds-brain, streams the \
bytes to it through the project's existing resumable uploader from the path \
named here, and finalises the catalogue row against that digest. Ingest is \
never implicit: it needs --yes, and a sensitivity is never inferred as open — \
the default is the folder's default or internal, and only a stricter class \
may be named here. Headless: the file is read on the host running `ds`; no \
window is involved. Files up to 256 MiB.",
    chapter: Chapter::Assets,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PATH_ARG, FOLDER_ARG, SENSITIVITY_ARG, LANE_ARG, PROJECT_ARG],
    output: "\
`asset` — the created row, with its `asset_id`, `digest`, `kind`, `format`, \
`folder` and `sensitivity`.",
    examples: &[Example {
        command: "ds assets ingest --project <exact-id> --path /home/me/Documents/EPC-Lot3-signed.pdf --folder contracts/2026/epc --sensitivity confidential --yes",
        note: "The row's asset_id feeds classify, attach and read from here on.",
        runnable: false,
    }],
    refusals: &crate::refusals::<27>(&[
        crate::INVALID_SOURCE_PATH,
        crate::INVALID_FOLDER_PATH,
        crate::CONFIRMATION_REQUIRED,
        crate::UNKNOWN_FOLDER,
        crate::ASSETS_UNREADABLE,
    ]),
    reference: Some("docs/reference/assets.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
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
/// relative one — a receipt that names an absolute path is one a reader can
/// check. A directory is refused rather than walked: ingest is one file,
/// explicitly.
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
        return Err(refuse("is not absolute; name the file so the receipt can"));
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
    let lane = inputs.value("lane").unwrap_or("stable");
    let project = inputs.require("project")?;
    let path = Path::new(arguments["path"].as_str().unwrap_or_default());
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            Failure::invalid("invalid_source_path", "`--path` has no file name")
                .remedy(crate::INVALID_SOURCE_PATH.remedy)
        })?
        .to_owned();
    let folder_id = match arguments["folder"].as_str() {
        Some(folder) => Some(
            crate::folder_at(lane, project, folder)?["folder_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ),
        None => None,
    };

    // One streaming pass digests the whole file and keeps its head for the
    // kernel to recognise; the file is then rewound and streamed to the
    // upload target. A 200 MiB pack never sits in memory.
    let unreadable = |why: &str| {
        Failure::invalid("invalid_source_path", format!("`--path` {why}"))
            .remedy(crate::INVALID_SOURCE_PATH.remedy)
            .detail(json!({ "given": path.display().to_string() }))
    };
    let mut file = std::fs::File::open(path).map_err(|_| unreadable("could not be opened"))?;
    let mut hasher = Sha256::new();
    let mut head: Vec<u8> = Vec::with_capacity(RECOGNISE_HEAD_BYTES);
    let mut size: u64 = 0;
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| unreadable("could not be read"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        if head.len() < RECOGNISE_HEAD_BYTES {
            let take = (RECOGNISE_HEAD_BYTES - head.len()).min(read);
            head.extend_from_slice(&buffer[..take]);
        }
        size += read as u64;
        if size > MAX_INGEST_BYTES {
            return Err(Failure::invalid(
                "invalid_source_path",
                format!("`--path` is above the {MAX_INGEST_BYTES} byte ingest bound"),
            )
            .remedy("ingest a file of at most 256 MiB; a larger deliverable is a pack to split")
            .detail(json!({ "max": MAX_INGEST_BYTES })));
        }
    }
    if size == 0 {
        return Err(unreadable("is empty; there is nothing to ingest"));
    }
    let digest = format!("{:x}", hasher.finalize());
    let recognised = ds_command_kernel::assets::recognise(Some(&name), &head, size);
    file.seek(SeekFrom::Start(0))
        .map_err(|_| unreadable("could not be rewound for upload"))?;

    let request = IngestRequest {
        name,
        size,
        sha256: digest.clone(),
        // The catalogue takes the content type as a hint; the kernel's
        // recognition of the head is the authority, and it names no MIME.
        content_type: None,
        folder_id,
        sensitivity: arguments["sensitivity"].as_str().map(str::to_owned),
        kind: crate::enum_token(&recognised.kind),
        format: crate::enum_token(&recognised.format),
    };
    let report = ds_cli_auth::project_assets_for_project(
        lane,
        project,
        &CatalogueCommand::Ingest(request),
        Some(&mut file),
    )?;
    let mut answer = report.into_result();
    answer["bytes"] = json!(size);
    answer["digest"] = json!(format!("sha256:{digest}"));
    answer["recognised"] = serde_json::to_value(&recognised)
        .map_err(|error| Failure::internal(crate::ASSETS_UNREADABLE.code, error.to_string()))?;
    Ok(answer)
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

    /// A file that certainly exists on any machine building this crate, and
    /// the directory that holds it.
    const CRATE_DIR: &str = env!("CARGO_MANIFEST_DIR");

    fn manifest() -> String {
        format!("{CRATE_DIR}/Cargo.toml")
    }

    fn parse(tokens: &[&str]) -> Inputs {
        let mut tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        tokens.extend(["--project".to_string(), "test_project".to_string()]);
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
