//! Filesystem adapter for the native Rust photo owner. CLI and MCP share these commands.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::survey_photo::{self, Manifest, Receipt};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

const DOCUMENT: ds_cli_contract::spec::Refusal = ds_cli_contract::spec::Refusal {
    code: "invalid_document",
    when: "an image, rotation bundle, or output directory violates the bounded file contract",
    remedy: "use a readable JPEG/PNG or an unchanged rotation bundle and choose a new output directory",
};
const REFUSALS: &[ds_cli_contract::spec::Refusal] = &{
    const BASE: &[ds_cli_contract::spec::Refusal] = crate::project_forms::LIST_COMMAND.refusals;
    let mut list = [DOCUMENT; BASE.len() + 1];
    let mut i = 0;
    while i < BASE.len() {
        list[i] = BASE[i];
        i += 1;
    }
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
    summary: "Rotate a pinned survey photo into a local publication bundle.",
    purpose: "Downloads one generation-pinned original and uses the shared Rust image engine to rotate it and produce its JPEG thumbnail. Saves immutable bytes and a digest-pinned manifest. No desktop is needed and nothing is published until survey photo publish; review the local images first. The explicit project never changes the saved selection.",
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
            "New private directory for manifest.json, original.bin and thumbnail.jpeg.",
        )
        .required(),
        crate::LANE,
    ],
    output: "Prepared bundle path, project, source generations, image dimensions and byte digests; no signed URLs or image bytes in the response.",
    examples: &[Example {
        command: "ds survey photo rotate --project <project-id> --path <object-path> --degrees 90 --out ./rotation --lane canary --output json",
        note: "Prepare a clockwise quarter-turn for local review and later publication.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static PUBLISH_COMMAND: Command = Command {
    id: "survey.photo.publish",
    path: &["survey", "photo", "publish"],
    contract: 1,
    summary: "Publish the exact original and thumbnail in a rotation bundle.",
    purpose: "Verifies bundle digests, project, authenticated account and deployment audience. Uploads the exact saved bytes under storage-generation preconditions, then verifies the thumbnail against the published original generation. Retry the same bundle after an interrupted transfer; it never applies another rotation. Concurrent unrelated edits are refused.",
    chapter: Chapter::Survey,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        Arg::value(
            "bundle",
            "<directory>",
            "Reviewed directory produced by survey photo rotate.",
        )
        .required(),
        crate::LANE,
    ],
    output: "Verified publication receipt with project, object paths, both storage generations and digests. A failure keeps the bundle available for retry.",
    examples: &[Example {
        command: "ds survey photo publish --project <project-id> --bundle ./rotation --lane canary --yes --output json",
        note: "Publish the reviewed pair; retry this same command after a transient failure.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
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
pub fn rotate(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let root = Path::new(inputs.require("out")?);
    if root.exists() {
        return Err(invalid(
            "Choose a new output directory; existing bundles are never overwritten",
        ));
    }
    let receipt = ds_cli_auth::survey_photo(
        inputs.require("lane")?,
        inputs.require("project")?,
        survey_photo::Command::Prepare {
            object_path: inputs.require("path")?,
            degrees: inputs
                .require("degrees")?
                .parse()
                .map_err(|_| invalid("Choose 90, 180 or 270 degrees"))?,
            expected_generation: inputs.value("expected-generation"),
        },
    )?;
    let Receipt::Prepared(prepared) = receipt else {
        return Err(invalid("Native photo owner returned an unexpected receipt"));
    };
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
    Ok(json!({"publication":"prepared", "bundle": root, "manifest":prepared.manifest}))
}
pub fn publish(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let root = Path::new(inputs.require("bundle")?);
    let meta =
        fs::symlink_metadata(root).map_err(|_| invalid("Rotation bundle directory is missing"))?;
    if !meta.is_dir() {
        return Err(invalid("Rotation bundle must be a regular directory"));
    }
    let manifest: Manifest = serde_json::from_slice(&read(root, "manifest.json", 16 * 1024)?)
        .map_err(|_| invalid("Invalid rotation manifest"))?;
    let original = read(root, "original.bin", 50 * 1024 * 1024)?;
    let thumbnail = read(root, "thumbnail.jpeg", 50 * 1024 * 1024)?;
    survey_photo::validate_manifest(
        inputs.require("project")?,
        &manifest.account_id,
        &manifest,
        &original,
        &thumbnail,
    )
    .map_err(|_| invalid("Rotation bundle bytes or project differ from the manifest"))?;
    match ds_cli_auth::survey_photo(
        inputs.require("lane")?,
        inputs.require("project")?,
        survey_photo::Command::Publish {
            manifest: &manifest,
            original: &original,
            thumbnail: &thumbnail,
        },
    )? {
        Receipt::Published(value) => Ok(value),
        Receipt::Prepared(_) => Err(invalid("Native photo owner returned an unexpected receipt")),
    }
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
