//! Filesystem adapter for the native Rust photo owner. CLI and MCP share these commands.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::survey_photo::{self, Manifest, Receipt};
use ds_command_kernel::survey_moments::{self, MediaRecord, RotationSource, SyncState};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::moments;

const DOCUMENT: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: "invalid_document",
    when: "an image, rotation bundle, or output directory violates the bounded file contract",
    remedy: "use a readable JPEG/PNG or an unchanged rotation bundle and choose a new output directory",
};
const NOT_ROTATABLE: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: "not_rotatable",
    when: "the path is not a survey original of the project (a thumbnail, a URL, another project's photo) or the turn is not 90, 180 or 270",
    remedy: "pass the canonical original object path and a net clockwise quarter-turn",
};
const NOT_WAITING: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: "moment_not_waiting",
    when: "no rotation of that path is held waiting on this machine",
    remedy: "run ds survey photo rotate without --out first, or pass --bundle",
};
const REFUSALS: &[ds_cli_contract::spec::Refusal] = &{
    const BASE: &[ds_cli_contract::spec::Refusal] = crate::project_forms::LIST_COMMAND.refusals;
    let mut list = [DOCUMENT; BASE.len() + 5];
    let mut i = 0;
    while i < BASE.len() {
        list[i] = BASE[i];
        i += 1;
    }
    list[BASE.len() + 1] = NOT_ROTATABLE;
    list[BASE.len() + 2] = NOT_WAITING;
    list[BASE.len() + 3] = moments::ROOT_INVALID;
    list[BASE.len() + 4] = moments::STORE_UNREADABLE;
    list
};

const PROJECT: Arg = Arg::value(
    "project",
    "<project-id>",
    "Explicit project containing the original photo.",
)
.required();
pub static ROTATE_COMMAND: Command = Command {
    id: "survey.photo.rotate",
    path: &["survey", "photo", "rotate"],
    contract: 1,
    summary: "Rotate a survey photo: held locally first, published after.",
    purpose: "The one rotation, with the properties panel's semantics: only a stored original of the project, never a thumbnail; a net clockwise quarter-turn applied once to the generation-pinned source through the shared Rust image engine, which regenerates the JPEG thumbnail. Without --out the result is held in this machine's survey-media store (survey moments list shows it `waiting`; a further turn of a waiting photo turns the held bytes and supersedes them; a full circle discards the wait) and is published by survey photo publish --path. With --out it is a private bundle for survey photo publish --bundle. Nothing reaches the bucket here.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        Arg::value(
            "path",
            "<object-path>",
            "Canonical original survey object path, not a URL or thumbnail.",
        )
        .required(),
        Arg::value(
            "degrees",
            "<90|180|270>",
            "Clockwise rotation relative to the pinned source.",
        )
        .required()
        .choices(&["90", "180", "270"]),
        Arg::value(
            "expected-generation",
            "<generation>",
            "Refuse if the source differs from the image you inspected.",
        ),
        Arg::value(
            "out",
            "<new-directory>",
            "Private bundle directory instead of this machine's survey-media store.",
        ),
        moments::SERVER_STATE_DIR_ARG,
        crate::LANE,
    ],
    output: "`publication` (held|prepared|discarded), `bundle` path, `manifest` (project, generations, dimensions, digests); no signed URLs or image bytes.",
    examples: &[
        Example {
            command: "ds survey photo rotate --project <project-id> --path <object-path> --degrees 90 --lane canary --output json",
            note: "Turn clockwise; held waiting on this machine until published.",
            runnable: false,
        },
        Example {
            command: "ds survey photo rotate --project <project-id> --path <object-path> --degrees 270 --out ./rotation --lane canary --output json",
            note: "Counter-clockwise, as a private bundle for review.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[
        "moments",
        "turn photo",
        "orientation",
        "sideways picture",
        "rotate image",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PUBLISH_COMMAND: Command = Command {
    id: "survey.photo.publish",
    path: &["survey", "photo", "publish"],
    contract: 1,
    summary: "Publish a held or bundled rotation: the exact bytes, once.",
    purpose: "Verifies digests, project, authenticated account and deployment audience, uploads the exact saved bytes under storage-generation preconditions, then verifies the thumbnail against the published original generation. --path publishes the rotation this machine holds waiting and marks it synced; --bundle publishes a private bundle. Retry after an interrupted transfer; it never applies another rotation. Concurrent unrelated edits are refused.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        Arg::value(
            "path",
            "<object-path>",
            "Original object path whose rotation this machine holds waiting.",
        ),
        Arg::value(
            "bundle",
            "<directory>",
            "Private directory produced by survey photo rotate --out.",
        ),
        moments::SERVER_STATE_DIR_ARG,
        crate::LANE,
    ],
    output: "Verified publication receipt with project, object paths, both storage generations and digests. A failure keeps the held bytes for retry.",
    examples: &[
        Example {
            command: "ds survey photo publish --project <project-id> --path <object-path> --lane canary --yes --output json",
            note: "Publish the waiting rotation; retry this same command after a transient failure.",
            runnable: false,
        },
        Example {
            command: "ds survey photo publish --project <project-id> --bundle ./rotation --lane canary --yes --output json",
            note: "Publish a reviewed private bundle.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn invalid(message: &str) -> Failure {
    Failure::invalid("invalid_document", message).remedy(DOCUMENT.remedy)
}
fn write_new(root: &Path, name: &str, bytes: &[u8]) -> Result<(), Failure> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(root.join(name))
        .map_err(|_| invalid("Cannot create rotation bundle file"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| invalid("Cannot persist rotation bundle file"))
}
fn read(root: &Path, name: &str, limit: usize) -> Result<Vec<u8>, Failure> {
    let path = root.join(name);
    let meta =
        fs::symlink_metadata(&path).map_err(|_| invalid("Rotation bundle file is missing"))?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err(invalid("Rotation bundle requires bounded regular files"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|_| invalid("Cannot open rotation bundle file"))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("Cannot read rotation bundle file"))?;
    if bytes.len() > limit {
        return Err(invalid("Rotation bundle file exceeds its byte bound"));
    }
    Ok(bytes)
}
fn not_rotatable(refusal: survey_moments::Refusal) -> Failure {
    Failure::invalid("not_rotatable", refusal.message).remedy(NOT_ROTATABLE.remedy)
}

fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Write one bundle into a fresh directory.
fn write_bundle(root: &Path, prepared: &survey_photo::Prepared) -> Result<(), Failure> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(root)
        .map_err(|_| invalid("Cannot create new rotation bundle directory"))?;
    write_new(root, "original.bin", &prepared.original)?;
    write_new(root, "thumbnail.jpeg", &prepared.thumbnail)?;
    let manifest = serde_json::to_vec_pretty(&prepared.manifest)
        .map_err(|_| invalid("Cannot encode rotation manifest"))?;
    // The manifest is the completion marker: a partial directory is never publishable.
    write_new(root, "manifest.json", &manifest)?;
    Ok(())
}

/// Replace the held bundle at `target` with a freshly written one: staged
/// beside it, swapped in, the previous bytes removed after. A crash leaves
/// either the old bundle or the new one, never a partial.
fn replace_bundle(target: &Path, prepared: &survey_photo::Prepared) -> Result<(), Failure> {
    let parent = target
        .parent()
        .ok_or_else(|| invalid("Survey-media directory has no parent"))?;
    fs::create_dir_all(parent)
        .map_err(|_| moments::unreadable("Cannot create survey-media directory"))?;
    let staging = parent.join(format!(
        "{}.staging-{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("bundle"),
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&staging);
    write_bundle(&staging, prepared)?;
    let previous = parent.join(format!(
        "{}.previous-{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("bundle"),
        std::process::id()
    ));
    if target.exists() {
        fs::rename(target, &previous)
            .map_err(|_| moments::unreadable("Cannot retire the held bundle"))?;
    }
    fs::rename(&staging, target)
        .map_err(|_| moments::unreadable("Cannot place the held bundle"))?;
    let _ = fs::remove_dir_all(&previous);
    Ok(())
}

/// The manifest a held bundle carries: the exact `survey photo publish` input.
fn held_manifest(root: &Path) -> Result<Manifest, Failure> {
    serde_json::from_slice(&read(root, "manifest.json", 16 * 1024)?)
        .map_err(|_| invalid("Invalid rotation manifest"))
}

fn record_of(
    prepared: &survey_photo::Prepared,
    previous: Option<&MediaRecord>,
    now: u64,
) -> MediaRecord {
    let m = &prepared.manifest;
    MediaRecord {
        schema: survey_moments::RECORD_SCHEMA.into(),
        project: m.project_id.clone(),
        path: m.object_path.clone(),
        thumbnail_path: m.thumbnail_path.clone(),
        media_type: m.media_type.clone(),
        size: m.original_size as u64,
        sha256: m.original_sha256.clone(),
        thumbnail_size: m.thumbnail_size as u64,
        thumbnail_sha256: m.thumbnail_sha256.clone(),
        width: m.original_width,
        height: m.original_height,
        cached_at_ms: previous.map_or(now, |p| p.cached_at_ms),
        updated_at_ms: now,
        state: SyncState::Waiting,
        degrees: m.degrees,
        source_generation: m.expected_generation.clone(),
        published_generation: None,
        published_thumbnail_generation: None,
        account: m.account_id.clone(),
        entry: previous.and_then(|p| p.entry.clone()),
    }
}

pub fn rotate(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    let path = inputs.require("path")?;
    let degrees: i64 = inputs
        .require("degrees")?
        .parse()
        .map_err(|_| invalid("Choose 90, 180 or 270 degrees"))?;
    let out = inputs.value("out").map(PathBuf::from);
    if let Some(root) = &out
        && root.exists()
    {
        return Err(invalid(
            "Choose a new output directory; existing bundles are never overwritten",
        ));
    }
    // The store mode consults the held record; the private-bundle mode never
    // touches the store.
    let state = if out.is_none() {
        Some(moments::server_state(inputs)?)
    } else {
        None
    };
    let waiting = match &state {
        Some(state) => {
            moments::record(state, project, path)?.filter(|r| r.state == SyncState::Waiting)
        }
        None => None,
    };
    let rotation = survey_moments::rotation(project, path, degrees, waiting.is_some())
        .map_err(not_rotatable)?;

    let prepared: Box<survey_photo::Prepared> = match (rotation.source, &waiting, &state) {
        (RotationSource::Waiting, Some(record), Some(state)) => {
            // The properties panel's second case: turn the waiting bytes and
            // supersede them; the bucket is not re-read and the precondition
            // the publication replaces stays the one the first turn pinned.
            let root = moments::media_dir(state, project, path);
            let manifest = held_manifest(&root)?;
            let held = read(&root, "original.bin", 50 * 1024 * 1024)?;
            if hash(&held) != record.sha256 || manifest.original_sha256 != record.sha256 {
                return Err(moments::unreadable(
                    "Held rotation bytes differ from their record",
                ));
            }
            let net = (manifest.degrees + rotation.degrees) % 360;
            if net == 0 {
                // A full circle is back to the published image: nothing to wait for.
                let mut store = ds_sync_store::Store::open(&state.join("store.sqlite"))
                    .map_err(moments::unreadable)?;
                store
                    .survey_media_remove(project, &[path.to_string()])
                    .map_err(moments::unreadable)?;
                let _ = fs::remove_dir_all(&root);
                return Ok(
                    json!({"publication": "discarded", "project": project, "path": path,
                    "reason": "the net turn is a full circle; the published image stands"}),
                );
            }
            let bundle = survey_photo::rotate_local(&held, rotation.degrees)
                .map_err(|_| invalid("Held image is malformed or unsupported"))?;
            let mut manifest = manifest;
            manifest.degrees = net;
            manifest.media_type = bundle.media_type.into();
            manifest.original_size = bundle.original_bytes.len();
            manifest.thumbnail_size = bundle.thumbnail_bytes.len();
            manifest.original_sha256 = hash(&bundle.original_bytes);
            manifest.thumbnail_sha256 = hash(&bundle.thumbnail_bytes);
            manifest.original_width = bundle.original_width;
            manifest.original_height = bundle.original_height;
            manifest.thumbnail_width = bundle.thumbnail_width;
            manifest.thumbnail_height = bundle.thumbnail_height;
            Box::new(survey_photo::Prepared {
                manifest,
                original: bundle.original_bytes,
                thumbnail: bundle.thumbnail_bytes,
            })
        }
        _ => {
            let receipt = ds_cli_auth::survey_photo(
                lane,
                project,
                survey_photo::Command::Prepare {
                    object_path: &rotation.path,
                    degrees: rotation.degrees,
                    expected_generation: inputs.value("expected-generation"),
                },
            )?;
            let Receipt::Prepared(prepared) = receipt else {
                return Err(invalid("Native photo owner returned an unexpected receipt"));
            };
            prepared
        }
    };

    match (out, state) {
        (Some(root), _) => {
            write_bundle(&root, &prepared)?;
            Ok(json!({"publication":"prepared", "bundle": root, "manifest":prepared.manifest}))
        }
        (None, Some(state)) => {
            let fence = moments::fence(lane)?;
            let root = moments::media_dir(&state, project, path);
            replace_bundle(&root, &prepared)?;
            let record = record_of(&prepared, waiting.as_ref(), now_ms());
            let mut store = ds_sync_store::Store::open(&state.join("store.sqlite"))
                .map_err(moments::unreadable)?;
            store
                .survey_media_put(&fence, &record)
                .map_err(moments::unreadable)?;
            Ok(
                json!({"publication":"held", "state": "waiting", "bundle": root, "manifest": prepared.manifest,
                "next": format!("ds survey photo publish --project {project} --path {path} --lane {lane} --yes")}),
            )
        }
        (None, None) => unreachable!("the store mode resolved its state root"),
    }
}
pub fn publish(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let lane = inputs.require("lane")?;
    let project = inputs.require("project")?;
    // Either the private bundle named, or the rotation this machine holds
    // waiting for the path named; never both, never neither.
    let (root, held): (PathBuf, Option<(PathBuf, MediaRecord)>) =
        match (inputs.value("bundle"), inputs.value("path")) {
            (Some(bundle), None) => (PathBuf::from(bundle), None),
            (None, Some(path)) => {
                let state = moments::server_state(inputs)?;
                let record = moments::record(&state, project, path)?
                    .filter(|record| record.state == SyncState::Waiting)
                    .ok_or_else(|| {
                        Failure::conflict(
                            "moment_not_waiting",
                            format!(
                                "no rotation of {path} is waiting on this machine for {project}"
                            ),
                        )
                        .remedy(NOT_WAITING.remedy)
                    })?;
                (
                    moments::media_dir(&state, project, path),
                    Some((state, record)),
                )
            }
            _ => return Err(invalid("Pass exactly one of --path or --bundle")),
        };
    let meta =
        fs::symlink_metadata(&root).map_err(|_| invalid("Rotation bundle directory is missing"))?;
    if !meta.is_dir() {
        return Err(invalid("Rotation bundle must be a regular directory"));
    }
    let manifest = held_manifest(&root)?;
    let original = read(&root, "original.bin", 50 * 1024 * 1024)?;
    let thumbnail = read(&root, "thumbnail.jpeg", 50 * 1024 * 1024)?;
    survey_photo::validate_manifest(
        project,
        &manifest.account_id,
        &manifest,
        &original,
        &thumbnail,
    )
    .map_err(|_| invalid("Rotation bundle bytes or project differ from the manifest"))?;
    let value = match ds_cli_auth::survey_photo(
        lane,
        project,
        survey_photo::Command::Publish {
            manifest: &manifest,
            original: &original,
            thumbnail: &thumbnail,
        },
    )? {
        Receipt::Published(value) => value,
        Receipt::Prepared(_) => {
            return Err(invalid("Native photo owner returned an unexpected receipt"));
        }
    };
    if let Some((state, mut record)) = held {
        // The held bytes now ARE the head: the record settles to synced and
        // the bytes stay as this machine's copy (cleanable, not retained).
        record.state = SyncState::Synced;
        record.published_generation = value["generation"].as_str().map(str::to_string);
        record.published_thumbnail_generation =
            value["thumbnail_generation"].as_str().map(str::to_string);
        record.updated_at_ms = now_ms();
        let fence = moments::fence(lane)?;
        let mut store =
            ds_sync_store::Store::open(&state.join("store.sqlite")).map_err(moments::unreadable)?;
        store
            .survey_media_put(&fence, &record)
            .map_err(moments::unreadable)?;
    }
    Ok(value)
}
pub fn render(data: &Value) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(data).unwrap_or_default()
    )
}

pub static LOCAL_COMMAND: Command = Command {
    id: "survey.photo.rotate-local",
    path: &["survey", "photo", "rotate-local"],
    contract: 1,
    summary: "Rotate a local survey image with Rust, offline.",
    purpose: "Supports image-recognition workflows without a desktop or sign-in. Reads a bounded JPEG or PNG, applies the requested clockwise turn using the same native/WASM owner, and saves the processed image, thumbnail and dimensions into a new directory. This local result is not a governed cloud publication bundle; use survey photo rotate for a stored project photo.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "input",
            "<image-path>",
            "Local JPEG or PNG; at most 50 MiB.",
        )
        .required(),
        Arg::value(
            "degrees",
            "<90|180|270>",
            "Clockwise rotation relative to the supplied image.",
        )
        .required()
        .choices(&["90", "180", "270"]),
        Arg::value(
            "out",
            "<new-directory>",
            "New directory for the rotated image and thumbnail.",
        )
        .required(),
    ],
    output: "Local artifact paths, pixel dimensions and media type; no publication or image bytes in the response.",
    examples: &[Example {
        command: "ds survey photo rotate-local --input ./photo.jpg --degrees 90 --out ./rotated --output json",
        note: "Rotate a local photo for visual review without any cloud changes.",
        runnable: false,
    }],
    refusals: &[DOCUMENT],
    reference: Some("docs/reference/survey.md"),
    search: &[],
    requires: Requires::Server,
    availability: || ds_cli_contract::spec::Availability::Available,
};
pub fn rotate_local(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let input = Path::new(inputs.require("input")?);
    let root = Path::new(inputs.require("out")?);
    if root.exists() {
        return Err(invalid("Choose a new output directory"));
    }
    let name = input
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| invalid("Invalid image filename"))?;
    let bytes = read(
        input.parent().unwrap_or(Path::new(".")),
        name,
        50 * 1024 * 1024,
    )?;
    let degrees = inputs
        .require("degrees")?
        .parse()
        .map_err(|_| invalid("Choose 90, 180 or 270 degrees"))?;
    let image = survey_photo::rotate_local(&bytes, degrees).map_err(|_| {
        invalid("Image is malformed, unsupported, oversized or has an invalid angle")
    })?;
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(root)
        .map_err(|_| invalid("Cannot create image output directory"))?;
    let filename = if image.media_type == "image/png" {
        "rotated.png"
    } else {
        "rotated.jpeg"
    };
    write_new(root, filename, &image.original_bytes)?;
    write_new(root, "thumbnail.jpeg", &image.thumbnail_bytes)?;
    Ok(
        json!({"publication":"local_only", "image":root.join(filename), "thumbnail":root.join("thumbnail.jpeg"), "media_type":image.media_type,
        "width":image.original_width,"height":image.original_height,"thumbnail_width":image.thumbnail_width,"thumbnail_height":image.thumbnail_height,"degrees":degrees}),
    )
}
