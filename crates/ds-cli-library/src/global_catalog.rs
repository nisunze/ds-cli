//! The governed global DS Grid catalog, through the native user.
//!
//! This catalog is global: a library release or an example revision belongs to
//! the product, not to a project. Until 2026-09-18 every command here reached
//! it through the paired desktop, which held the same signed-in user and
//! posted the same bodies to the same gateway path — so a global,
//! project-free operation required an open window, and these commands declared
//! `Available` while refusing `not_paired` on any machine without one.
//!
//! The transport is now `ds-client-core`'s closed `grid_catalog` owner. Ids,
//! arguments and answers are unchanged; `--desktop-descriptor` is replaced by
//! `--lane`, the authority becomes the restored native user, and the two
//! composed operations the desktop performed — artifact upload and prepared
//! publication — are performed here, where reading a local directory is the
//! natural thing rather than the awkward one.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::grid_catalog::Command as Catalog;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const READ_ACTION: Arg = Arg {
    name: "action",
    kind: ArgKind::Value,
    value: "<action>",
    required: true,
    default: None,
    choices: &[
        "library-list",
        "library-read",
        "library-releases",
        "example-list",
        "example-revisions",
    ],
    summary: "Read-only exact catalog discovery action.",
};
const WRITE_ACTION: Arg = Arg {
    name: "action",
    kind: ArgKind::Value,
    value: "<action>",
    required: true,
    default: None,
    choices: &[
        "upload",
        "library-publish",
        "example-publish",
        "library-lifecycle",
        "example-lifecycle",
    ],
    summary: "Confirmation-required governed catalog write action.",
};
const PAYLOAD: Arg = Arg::value(
    "payload",
    "<json>",
    "Typed action body JSON (library, example, lifecycle, or fork fields).",
);
const PATH: Arg = Arg::value("path", "<path>", "Local artifact path for upload only.");
const PURPOSE: Arg = Arg::value(
    "purpose",
    "<purpose>",
    "Upload purpose: library_manifest, library_validation_report, library_asset, example_model, example_project, or example_preview.",
);
const VISIBILITY: Arg = Arg::value(
    "visibility",
    "<visibility>",
    "Upload visibility: public, organization, or private.",
);
/// Every refusal `ds-cli-auth`'s native user path can return. Composed rather
/// than copied: a new native refusal reaches these commands the moment the
/// owner declares it, which is what `refusal_coverage.rs` checks for.
const NATIVE: &[Refusal] = ds_cli_auth::PROJECT_LIST_COMMAND.refusals;

const PAYLOAD_REFUSAL: Refusal = Refusal {
    code: "catalog_payload_invalid",
    when: "--payload is absent, is not JSON, or is not one object",
    remedy: "pass one object matching the selected action",
};
const ACTION_REFUSAL: Refusal = Refusal {
    code: "catalog_action_invalid",
    when: "the action is outside the set this command declares",
    remedy: "choose one action from --help",
};
const ARTIFACT_REFUSAL: Refusal = Refusal {
    code: "catalog_artifact_unreadable",
    when: "a local artifact or prepared document cannot be read, is empty, or exceeds its bound",
    remedy: "pass an existing readable file under 256 MiB, inside the prepared directory",
};
const PREPARED_REFUSAL: Refusal = Refusal {
    code: "catalog_prepared_invalid",
    when: "the prepared document is missing a required field, or names a path outside its directory",
    remedy: "fix the named field in library.json or example.json and retry",
};
const UPLOAD_REFUSAL: Refusal = Refusal {
    code: "catalog_upload_failed",
    when: "the governed artifact transfer did not complete or verify",
    remedy: "retry the same artifact; the session is minted fresh each attempt",
};

/// How many refusals the native user path declares today. A change in the
/// owner breaks the array lengths below at compile time rather than leaving an
/// undocumented code, and `native_refusal_count_is_the_owner_s` names it.
const NATIVE_COUNT: usize = 16;

/// This domain's own refusals, then the native user's, in one array.
///
/// `OWN + NATIVE_COUNT` cannot be written as a const-generic expression, so
/// `TOTAL` is passed explicitly and the fill below refuses to compile if it
/// disagrees: a wrong total either leaves a placeholder in the tail or indexes
/// past the end, and `every_composed_set_ends_with_the_native_refusals` reads
/// the result back.
const fn with_native<const OWN: usize, const TOTAL: usize>(
    own: [Refusal; OWN],
) -> [Refusal; TOTAL] {
    let mut all = [PAYLOAD_REFUSAL; TOTAL];
    let mut index = 0;
    while index < OWN {
        all[index] = own[index];
        index += 1;
    }
    let mut native = 0;
    while native < NATIVE_COUNT {
        all[OWN + native] = NATIVE[native];
        native += 1;
    }
    all
}

const READ_SET: [Refusal; 18] = with_native::<2, 18>([PAYLOAD_REFUSAL, ACTION_REFUSAL]);
const READ_REFUSALS: &[Refusal] = &READ_SET;
const WRITE_SET: [Refusal; 21] = with_native::<5, 21>([
    PAYLOAD_REFUSAL,
    ACTION_REFUSAL,
    ARTIFACT_REFUSAL,
    UPLOAD_REFUSAL,
    PREPARED_REFUSAL,
]);
const WRITE_REFUSALS: &[Refusal] = &WRITE_SET;
const FORK_SET: [Refusal; 17] = with_native::<1, 17>([PAYLOAD_REFUSAL]);
const FORK_REFUSALS: &[Refusal] = &FORK_SET;
const REFUSALS: &[Refusal] = NATIVE;

const LANE: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
const PREPARED: Arg = Arg::value(
    "prepared",
    "<directory>",
    "Directory containing the typed library.json or example.json preparation document and its named local files.",
);
const LIBRARY_ID: Arg = Arg::value("library-id", "<library-id>", "Global library id.");
const EXAMPLE_ID: Arg = Arg::value("example-id", "<example-id>", "Global example id.");
const EXPECTED_HEAD_RELEASE: Arg = Arg::value(
    "expected-head-release",
    "<release-id>",
    "Exact current library head release id; refuses if it moved.",
);
const EXPECTED_HEAD_REVISION: Arg = Arg::value(
    "expected-head-revision",
    "<revision-id>",
    "Exact current example head revision id; refuses if it moved.",
);
const EXPECTED_LIFECYCLE: Arg = Arg::value(
    "expected-lifecycle",
    "<state>",
    "Current lifecycle fence: active, archived, or deprecated.",
);
const LIFECYCLE: Arg = Arg::value(
    "lifecycle",
    "<state>",
    "Target lifecycle: active, archived, or deprecated.",
);
pub static READ_COMMAND: Command = Command {
    id: "library.global.read",
    path: &["library", "global", "read"],
    contract: 1,
    summary: "List or inspect exact global catalog libraries and examples.",
    purpose: "Map-independent signed-in catalog discovery only.",
    chapter: Chapter::PlsCadd,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[READ_ACTION, PAYLOAD, LANE],
    output: "Bounded exact library/example heads or immutable release/revision histories.",
    examples: &[Example {
        command: "ds library global read --action library-releases --payload '{\"library_id\":\"rw-pls-cadd-structures\"}' --output json",
        note: "List the immutable releases of one governed global library without opening a map.",
        runnable: false,
    }],
    refusals: READ_REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static WRITE_COMMAND: Command = Command {
    id: "library.global.write",
    path: &["library", "global", "write"],
    contract: 1,
    summary: "Legacy payload route for governed global catalog writes.",
    purpose: "Confirmation-required publisher action. Upload session URIs are never emitted.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[WRITE_ACTION, PAYLOAD, PATH, PURPOSE, VISIBILITY, LANE],
    output: "Artifact pin, immutable published record, or fenced lifecycle receipt.",
    examples: &[Example {
        command: "ds library global write --action library-lifecycle --payload '{\"library_id\":\"rw-pls-cadd-structures\",\"expected_head_release_id\":\"2026.08\",\"lifecycle\":\"archived\"}' --yes --output json",
        note: "Archive the current library head with an optimistic head fence; immutable releases remain readable.",
        runnable: false,
    }],
    refusals: WRITE_REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static FORK_COMMAND: Command = Command {
    id: "library.global.fork-example",
    path: &["library", "global", "fork-example"],
    contract: 1,
    summary: "Fork one exact global example revision into a project model.",
    purpose: "Project-authorized, map-independent exact fork; no source reupload/copy.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[PAYLOAD, LANE],
    output: "The new immutable project model version with server-derived global provenance.",
    examples: &[Example {
        command: "ds library global fork-example --payload '{\"project_id\":\"my-project\",\"fork\":{\"example_id\":\"karongi-mv\",\"example_revision_id\":\"2026.08\",\"expected_head_revision_id\":\"2026.08\",\"model_id\":\"karongi-copy\",\"revision_id\":\"v1\",\"display_name\":\"Karongi governed copy\",\"model_kind\":\"mv_line\",\"model_schema_version\":\"1\",\"engine_version\":\"pls-cadd-pinned\",\"reason\":\"Start from the proven global example\"}}' --yes --output json",
        note: "Fork one exact active global example revision into an authorized project without copying or re-uploading its source object.",
        runnable: false,
    }],
    refusals: FORK_REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static UPLOAD_COMMAND: Command = Command {
    id: "library.global.upload",
    path: &["library", "global", "upload"],
    contract: 1,
    summary: "Upload one typed global catalogue artifact from a local file.",
    purpose: "Publisher-only, map-independent content-addressed upload. It returns an immutable artifact pin and never exposes the resumable session URI, publishes a release, or evaluates a native model.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[PATH, PURPOSE, VISIBILITY, LANE],
    output: "One canonical digest, object and byte-length artifact pin.",
    examples: &[Example {
        command: "ds library global upload --path ./pls-cadd/criteria.cri --purpose library_asset --visibility organization --yes --output json",
        note: "Upload one opaque library asset; it is not a solver approval.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PUBLISH_LIBRARY_COMMAND: Command = Command {
    id: "library.global.publish-library",
    path: &["library", "global", "publish-library"],
    contract: 1,
    summary: "Create or advance a governed global library from a prepared directory.",
    purpose: "Publisher-only, map-independent publication. The paired desktop reads library.json and its explicitly named files, uploads manifest, validation evidence and typed assets under closed purposes, then publishes one immutable release. It never overwrites a release or approves a native solver result.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[PREPARED, LANE],
    output: "The mutable library head pointing to one newly published or idempotently retried immutable release.",
    examples: &[Example {
        command: "ds library global publish-library --prepared ./karongi-library --yes --output json",
        note: "Publish the typed library.json preparation directory without a raw API JSON argument.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PUBLISH_EXAMPLE_COMMAND: Command = Command {
    id: "library.global.publish-example",
    path: &["library", "global", "publish-example"],
    contract: 1,
    summary: "Create or advance a governed global example from a prepared directory.",
    purpose: "Publisher-only, map-independent publication. The paired desktop reads example.json and its named model, project-plane and preview files, uploads them under closed purposes, and pins one exact library release. It does not run or approve a solver.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[PREPARED, LANE],
    output: "The mutable example head pointing to one newly published or idempotently retried immutable revision.",
    examples: &[Example {
        command: "ds library global publish-example --prepared ./karongi-example --yes --output json",
        note: "Publish the typed example.json preparation directory without a raw API JSON argument.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static LIBRARY_LIFECYCLE_COMMAND: Command = Command {
    id: "library.global.library-lifecycle",
    path: &["library", "global", "library-lifecycle"],
    contract: 1,
    summary: "Archive, deprecate, or restore a global library head with fences.",
    purpose: "Publisher-only mutable-head management. It requires both exact head and lifecycle fences; immutable releases are never changed or deleted.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        LIBRARY_ID,
        EXPECTED_HEAD_RELEASE,
        EXPECTED_LIFECYCLE,
        LIFECYCLE,
        LANE,
    ],
    output: "The fenced library head lifecycle receipt.",
    examples: &[Example {
        command: "ds library global library-lifecycle --library-id karongi --expected-head-release r1 --expected-lifecycle active --lifecycle archived --yes --output json",
        note: "Archive a library head while retaining every immutable release.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static EXAMPLE_LIFECYCLE_COMMAND: Command = Command {
    id: "library.global.example-lifecycle",
    path: &["library", "global", "example-lifecycle"],
    contract: 1,
    summary: "Archive, deprecate, or restore a global example head with fences.",
    purpose: "Publisher-only mutable-head management. It requires both exact head and lifecycle fences; immutable example revisions are never changed or deleted.",
    chapter: Chapter::PlsCadd,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        EXAMPLE_ID,
        EXPECTED_HEAD_REVISION,
        EXPECTED_LIFECYCLE,
        LIFECYCLE,
        LANE,
    ],
    output: "The fenced example head lifecycle receipt.",
    examples: &[Example {
        command: "ds library global example-lifecycle --example-id karongi --expected-head-revision r1 --expected-lifecycle archived --lifecycle active --yes --output json",
        note: "Restore a governed example before it can seed a new project.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/library.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The largest artifact this owner will read into memory for a governed
/// upload. It matches the model package bound the same catalog accepts.
const MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

fn payload_object(inputs: &Inputs) -> Result<Value, Failure> {
    let raw = inputs.value("payload").ok_or_else(|| {
        Failure::invalid("catalog_payload_invalid", "--payload is required").remedy(PAYLOAD.summary)
    })?;
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| Failure::invalid("catalog_payload_invalid", "--payload must be JSON"))?;
    if !value.is_object() {
        return Err(Failure::invalid(
            "catalog_payload_invalid",
            "--payload must be one JSON object",
        ));
    }
    Ok(value)
}

/// A required string field of the payload, named in the refusal when absent so
/// a caller learns WHICH field the action wanted.
fn field(payload: &Value, name: &str) -> Result<String, Failure> {
    payload[name]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            Failure::invalid(
                "catalog_payload_invalid",
                format!("--payload must carry a non-empty `{name}`"),
            )
            .remedy(format!("add \"{name}\" to the payload object"))
        })
}

fn optional(payload: &Value, name: &str) -> Option<String> {
    payload[name]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn catalog(inputs: &Inputs, command: Catalog) -> Result<Value, Failure> {
    ds_cli_auth::grid_catalog(inputs.require("lane")?, &command)
}

fn unknown_action(action: &str) -> Failure {
    Failure::invalid(
        "catalog_action_invalid",
        format!("`{action}` is not an action this command offers"),
    )
    .remedy("choose one action from --help")
}

pub fn run_read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let action = inputs.require("action")?;
    // Only `library-list` and `example-list` need no selection, so the payload
    // is read once and only where an id is required.
    let command = match action {
        "library-list" => Catalog::ListLibraries,
        "example-list" => Catalog::ListExamples,
        "library-read" => {
            let payload = payload_object(inputs)?;
            Catalog::GetLibrary {
                library: field(&payload, "library_id")?,
                release: optional(&payload, "release_id"),
            }
        }
        "library-releases" => Catalog::ListLibraryReleases {
            library: field(&payload_object(inputs)?, "library_id")?,
        },
        "example-revisions" => Catalog::ListExampleRevisions {
            example: field(&payload_object(inputs)?, "example_id")?,
        },
        other => return Err(unknown_action(other)),
    };
    catalog(inputs, command)
}

pub fn run_write(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let action = inputs.require("action")?;
    match action {
        "upload" => run_upload(inputs, context),
        "library-publish" => catalog(
            inputs,
            Catalog::PublishLibraryRelease {
                library: payload_object(inputs)?,
            },
        ),
        "example-publish" => catalog(
            inputs,
            Catalog::PublishExampleRevision {
                example: payload_object(inputs)?,
            },
        ),
        "library-lifecycle" => catalog(
            inputs,
            Catalog::SetLibraryLifecycle {
                lifecycle: payload_object(inputs)?,
            },
        ),
        "example-lifecycle" => catalog(
            inputs,
            Catalog::SetExampleLifecycle {
                lifecycle: payload_object(inputs)?,
            },
        ),
        other => Err(unknown_action(other)),
    }
}

/// Fork one exact governed example revision into a project model.
///
/// The project is named in the request, never taken from a saved selection:
/// `grid_models_for_project` authorizes exactly the project the caller wrote.
pub fn run_fork(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let payload = payload_object(inputs)?;
    let project = field(&payload, "project_id")?;
    let fork = payload
        .get("fork")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| {
            Failure::invalid(
                "catalog_payload_invalid",
                "--payload must carry a `fork` object",
            )
            .remedy("nest the fork request under \"fork\"")
        })?;
    ds_cli_auth::grid_models_for_project(
        inputs.require("lane")?,
        &project,
        &ds_cli_auth::GridModelsCommand::ForkExample { fork },
    )
    .map(|receipt| receipt.data)
}

/// Read one local artifact, seal it, and hand the governed catalog its bytes.
///
/// `start_artifact_upload` answers with the artifact pin and, when the catalog
/// does not already hold these exact bytes, one storage session. The session
/// never leaves this function: the caller receives the pin, which is what a
/// publication request carries.
fn upload_artifact(
    inputs: &Inputs,
    path: &std::path::Path,
    purpose: &str,
    visibility: &str,
) -> Result<Value, Failure> {
    let metadata = std::fs::metadata(path).map_err(|error| artifact_unreadable(path, error))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES {
        return Err(Failure::invalid(
            "catalog_artifact_unreadable",
            format!(
                "{} is not a readable file within the {} MiB bound",
                path.display(),
                MAX_ARTIFACT_BYTES / (1024 * 1024)
            ),
        )
        .remedy("pass one existing non-empty artifact file"));
    }
    let bytes = std::fs::read(path).map_err(|error| artifact_unreadable(path, error))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let started = catalog(
        inputs,
        Catalog::StartArtifactUpload {
            upload: json!({
                "digest": digest,
                "byte_length": bytes.len(),
                "content_type": "application/octet-stream",
                "purpose": purpose,
                "visibility": visibility,
            }),
        },
    )?;
    let artifact = started
        .get("artifact")
        .cloned()
        .ok_or_else(|| upload_failed("the catalog returned no artifact pin"))?;
    if started["already_exists"].as_bool() == Some(true) {
        return Ok(artifact);
    }
    let session = started["session_uri"]
        .as_str()
        .filter(|uri| !uri.is_empty())
        .ok_or_else(|| upload_failed("the catalog returned no usable upload session"))?;
    ds_sync_runtime::native_transfer::drive_verified_output(
        session,
        bytes.len() as u64,
        &digest,
        std::io::Cursor::new(&bytes),
        None,
        started["max_chunk_size"].as_u64(),
        &|| false,
        &mut |_| {},
    )
    .map_err(|message| upload_failed(&message))?;
    Ok(artifact)
}

fn artifact_unreadable(path: &std::path::Path, error: std::io::Error) -> Failure {
    Failure::invalid(
        "catalog_artifact_unreadable",
        format!("{} cannot be read: {error}", path.display()),
    )
    .remedy("pass one existing readable artifact path")
}

fn upload_failed(message: &str) -> Failure {
    Failure::unavailable("catalog_upload_failed", message.to_owned())
        .remedy("retry the same artifact; a fresh session is minted each attempt")
}

pub fn run_upload(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = std::path::PathBuf::from(inputs.require("path")?);
    upload_artifact(
        inputs,
        &path,
        inputs.require("purpose")?,
        inputs.require("visibility")?,
    )
}

pub fn run_publish_library(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    publish_prepared(inputs, Kind::Library)
}

pub fn run_publish_example(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    publish_prepared(inputs, Kind::Example)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Library,
    Example,
}

impl Kind {
    const fn leaf(self) -> &'static str {
        match self {
            Self::Library => "library.json",
            Self::Example => "example.json",
        }
    }
    const fn record(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::Example => "example",
        }
    }
}

fn prepared_invalid(field: &str, why: &str) -> Failure {
    Failure::invalid("catalog_prepared_invalid", format!("{field} {why}"))
        .remedy("fix that field in the prepared document and retry")
}

/// Resolve one named file inside the prepared directory.
///
/// A prepared document names its files by relative path, so this is the one
/// place a document could reach outside its own directory. It cannot: an
/// absolute path, a backslash, an empty segment, `.` or `..` is refused before
/// anything is read.
fn prepared_path(
    root: &std::path::Path,
    source: &Value,
    field_name: &str,
) -> Result<std::path::PathBuf, Failure> {
    let relative = source["path"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| prepared_invalid(&format!("{field_name}.path"), "is required"))?;
    if relative.starts_with('/')
        || relative.contains('\\')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(prepared_invalid(
            &format!("{field_name}.path"),
            "must name a file inside the prepared directory",
        ));
    }
    Ok(root.join(relative))
}

/// Upload one artifact named by the prepared document and return its pin.
fn prepared_artifact(
    inputs: &Inputs,
    root: &std::path::Path,
    source: &Value,
    purpose: &str,
    visibility: &str,
    field_name: &str,
) -> Result<Value, Failure> {
    if !source.is_object() {
        return Err(prepared_invalid(field_name, "must be an object"));
    }
    let path = prepared_path(root, source, field_name)?;
    upload_artifact(inputs, &path, purpose, visibility)
}

/// The source path is the operator's local fact; the published record carries
/// the artifact pin instead.
fn without_path(source: &Value, artifact: Value) -> Value {
    let mut row = source.clone();
    if let Some(object) = row.as_object_mut() {
        object.remove("path");
        object.insert("artifact".into(), artifact);
    }
    row
}

fn publish_prepared(inputs: &Inputs, kind: Kind) -> Result<Value, Failure> {
    let root = std::path::PathBuf::from(inputs.require("prepared")?);
    let document = root.join(kind.leaf());
    let bytes = std::fs::read(&document).map_err(|error| artifact_unreadable(&document, error))?;
    let config: Value = serde_json::from_slice(&bytes)
        .map_err(|_| prepared_invalid(kind.leaf(), "must contain one JSON object"))?;
    if !config.is_object() {
        return Err(prepared_invalid(
            kind.leaf(),
            "must contain one JSON object",
        ));
    }
    let visibility = config["visibility"]
        .as_str()
        .map(str::trim)
        .filter(|value| ["public", "organization", "private"].contains(value))
        .ok_or_else(|| prepared_invalid("visibility", "must be public, organization, or private"))?
        .to_owned();
    let record = config[kind.record()].clone();
    if !record.is_object() {
        return Err(prepared_invalid(kind.record(), "must be an object"));
    }
    let revision = if record["release"].is_object() {
        record["release"].clone()
    } else {
        record["revision"].clone()
    };
    if !revision.is_object() {
        return Err(prepared_invalid(
            &format!("{}.release", kind.record()),
            "must be an object",
        ));
    }

    let mut published = revision.clone();
    let object = published
        .as_object_mut()
        .expect("prepared revision is an object");
    match kind {
        Kind::Library => {
            let assets = revision["assets"].as_array().cloned().unwrap_or_default();
            let mut uploaded = Vec::with_capacity(assets.len());
            for (index, asset) in assets.iter().enumerate() {
                let name = format!("release.assets[{index}]");
                let artifact =
                    prepared_artifact(inputs, &root, asset, "library_asset", &visibility, &name)?;
                uploaded.push(without_path(asset, artifact));
            }
            object.insert(
                "manifest".into(),
                prepared_artifact(
                    inputs,
                    &root,
                    &revision["manifest"],
                    "library_manifest",
                    &visibility,
                    "release.manifest",
                )?,
            );
            object.insert(
                "validation_report".into(),
                prepared_artifact(
                    inputs,
                    &root,
                    &revision["validation_report"],
                    "library_validation_report",
                    &visibility,
                    "release.validation_report",
                )?,
            );
            object.insert("assets".into(), Value::Array(uploaded));
        }
        Kind::Example => {
            let artifacts = revision["artifacts"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mut uploaded = Vec::with_capacity(artifacts.len());
            for (index, artifact) in artifacts.iter().enumerate() {
                let name = format!("revision.artifacts[{index}]");
                let purpose = if artifact["kind"] == "project" {
                    "example_project"
                } else {
                    "example_model"
                };
                let pin = prepared_artifact(inputs, &root, artifact, purpose, &visibility, &name)?;
                uploaded.push(without_path(artifact, pin));
            }
            let previews = revision["previews"].as_array().cloned().unwrap_or_default();
            let mut uploaded_previews = Vec::with_capacity(previews.len());
            for (index, preview) in previews.iter().enumerate() {
                let name = format!("revision.previews[{index}]");
                uploaded_previews.push(prepared_artifact(
                    inputs,
                    &root,
                    preview,
                    "example_preview",
                    &visibility,
                    &name,
                )?);
            }
            object.insert(
                "model".into(),
                prepared_artifact(
                    inputs,
                    &root,
                    &revision["model"],
                    "example_model",
                    &visibility,
                    "revision.model",
                )?,
            );
            object.insert("previews".into(), Value::Array(uploaded_previews));
            object.insert("artifacts".into(), Value::Array(uploaded));
        }
    }

    let mut request = record.clone();
    let body = request
        .as_object_mut()
        .expect("prepared record is an object");
    body.insert("visibility".into(), Value::String(visibility));
    body.remove("revision");
    body.insert(
        match kind {
            Kind::Library => "release".into(),
            Kind::Example => "revision".into(),
        },
        published,
    );
    catalog(
        inputs,
        match kind {
            Kind::Library => Catalog::PublishLibraryRelease { library: request },
            Kind::Example => Catalog::PublishExampleRevision { example: request },
        },
    )
}

pub fn run_library_lifecycle(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    catalog(
        inputs,
        Catalog::SetLibraryLifecycle {
            lifecycle: json!({
                "library_id": inputs.require("library-id")?,
                "expected_head_release_id": inputs.require("expected-head-release")?,
                "expected_lifecycle": inputs.require("expected-lifecycle")?,
                "lifecycle": inputs.require("lifecycle")?,
            }),
        },
    )
}

pub fn run_example_lifecycle(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    catalog(
        inputs,
        Catalog::SetExampleLifecycle {
            lifecycle: json!({
                "example_id": inputs.require("example-id")?,
                "expected_head_revision_id": inputs.require("expected-head-revision")?,
                "expected_lifecycle": inputs.require("expected-lifecycle")?,
                "lifecycle": inputs.require("lifecycle")?,
            }),
        },
    )
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_else(|_| "catalog result".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_and_publisher_actions_are_disjoint() {
        let read = READ_COMMAND.args[0].choices;
        let write = WRITE_COMMAND.args[0].choices;

        assert!(read.iter().all(|action| !write.contains(action)));
        assert!(read.contains(&"library-list"));
        assert!(!read.contains(&"library-publish"));
        assert!(write.contains(&"library-publish"));
        assert!(!write.contains(&"library-list"));
    }

    #[test]
    fn exact_project_fork_has_no_action_multiplexer() {
        assert_eq!(FORK_COMMAND.id, "library.global.fork-example");
        assert!(FORK_COMMAND.args.iter().all(|arg| arg.name != "action"));
    }

    #[test]
    fn prepared_publication_and_lifecycle_are_not_raw_payload_commands() {
        for command in [
            &UPLOAD_COMMAND,
            &PUBLISH_LIBRARY_COMMAND,
            &PUBLISH_EXAMPLE_COMMAND,
            &LIBRARY_LIFECYCLE_COMMAND,
            &EXAMPLE_LIFECYCLE_COMMAND,
        ] {
            assert!(command.args.iter().all(|arg| arg.name != "payload"));
        }
        assert!(PURPOSE.summary.contains("library_asset"));
        assert!(PURPOSE.summary.contains("example_project"));
    }

    /// The whole point of the move: no command here asks for a window.
    #[test]
    fn every_global_catalog_command_is_a_native_user_command() {
        for command in [
            &READ_COMMAND,
            &WRITE_COMMAND,
            &FORK_COMMAND,
            &UPLOAD_COMMAND,
            &PUBLISH_LIBRARY_COMMAND,
            &PUBLISH_EXAMPLE_COMMAND,
            &LIBRARY_LIFECYCLE_COMMAND,
            &EXAMPLE_LIFECYCLE_COMMAND,
        ] {
            assert_eq!(
                command.authority,
                Authority::HeadlessUser,
                "{} still declares a desktop authority",
                command.id
            );
            assert!(
                command.args.iter().any(|arg| arg.name == "lane"),
                "{} cannot select its authentication lane",
                command.id
            );
            assert!(
                command
                    .args
                    .iter()
                    .all(|arg| arg.name != "desktop-descriptor"),
                "{} still accepts a paired descriptor",
                command.id
            );
            assert!(
                command
                    .refusals
                    .iter()
                    .all(|refusal| refusal.code != "not_paired"),
                "{} still documents a pairing refusal",
                command.id
            );
        }
    }

    #[test]
    fn native_refusal_count_is_the_owner_s() {
        // The composed arrays below have fixed lengths, so a refusal added to
        // the native owner must be accounted for here rather than dropped.
        assert_eq!(NATIVE.len(), NATIVE_COUNT);
    }

    #[test]
    fn every_composed_set_ends_with_the_native_refusals() {
        for (set, own) in [
            (READ_REFUSALS, 2usize),
            (WRITE_REFUSALS, 5),
            (FORK_REFUSALS, 1),
        ] {
            assert_eq!(set.len(), own + NATIVE_COUNT);
            for (index, refusal) in NATIVE.iter().enumerate() {
                assert_eq!(
                    set[own + index].code,
                    refusal.code,
                    "composed set lost a native refusal"
                );
            }
        }
    }

    /// A prepared document names its files by relative path. This is the one
    /// place it could reach outside its own directory, so the refusal is
    /// tested with the shapes that would try.
    #[test]
    fn a_prepared_path_cannot_leave_its_directory() {
        let root = std::path::Path::new("/tmp/prepared");
        for bad in [
            json!({ "path": "/etc/passwd" }),
            json!({ "path": "../secret" }),
            json!({ "path": "assets/../../secret" }),
            json!({ "path": "assets\\windows" }),
            json!({ "path": "assets//model" }),
            json!({ "path": "./model" }),
            json!({ "path": "" }),
            json!({}),
        ] {
            assert!(
                prepared_path(root, &bad, "release.assets[0]").is_err(),
                "{bad} was accepted as a prepared path"
            );
        }
        assert_eq!(
            prepared_path(root, &json!({ "path": "assets/model.dsgrid" }), "a")
                .expect("a relative path inside the directory is accepted"),
            root.join("assets/model.dsgrid")
        );
    }

    #[test]
    fn a_published_record_carries_the_pin_and_not_the_operator_s_local_path() {
        let source = json!({ "path": "assets/a.dsgrid", "kind": "model", "label": "A" });
        let published = without_path(&source, json!({ "object": "ds-grid/objects/sha256/aa" }));
        assert!(published["path"].is_null(), "the local path was published");
        assert_eq!(published["kind"], "model");
        assert_eq!(published["label"], "A");
        assert_eq!(published["artifact"]["object"], "ds-grid/objects/sha256/aa");
    }

    #[test]
    fn a_payload_field_names_what_is_missing() {
        let payload = json!({ "release_id": "r1" });
        let refused = field(&payload, "library_id").unwrap_err();
        assert_eq!(refused.code(), "catalog_payload_invalid");
        assert!(
            format!("{refused:?}").contains("library_id"),
            "the refusal does not name the absent field"
        );
        assert_eq!(field(&payload, "release_id").expect("present"), "r1");
        assert_eq!(optional(&payload, "library_id"), None);
    }

    #[test]
    fn an_action_outside_the_declared_set_is_refused_before_any_call() {
        assert_eq!(
            unknown_action("library-delete").code(),
            "catalog_action_invalid"
        );
    }
}
