//! `ds design collisions detect` and `ds design collisions publish` — project
//! transformer-collision detection on this machine, and its governed save.
//!
//! Detection never runs in the cloud. `detect` reads the project's transformer
//! heads, acquires the saved rooms through the governed room hold (a held room
//! whose saved version is current is reused; offline, the held rooms are the
//! input) and hands them to the Reporter addon's fixed `detect-collisions`
//! task, whose request schema names the room layers it reads. Core links no
//! detection algorithm. `publish` carries one detect result, with the room
//! versions it was computed from, to ds-brain's storage action, which refuses a
//! result whose basis moved. Contract: ds-network-reporter
//! `docs/contracts/collisions.md`.

use std::ffi::OsString;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::transformer::{LANE_ARG, PROJECT_ARG};

/// Core's result document, written by `detect --out` and read by `publish`.
pub const RESULT_SCHEMA: &str = "ds.design-collisions-result/v1";
/// The bounded receipt of a written result file.
pub const FILE_SCHEMA: &str = "ds.design-collisions-file/v1";
const REQUEST_SCHEMA: &str = "ds.reporter.detect-collisions-request/v1";
const ENGINE_RESULT_SCHEMA: &str = "ds.reporter.detect-collisions-result/v1";
const PUBLICATION_SCHEMA: &str = "ds.collisions-publication/v1";
const SUBCOMMAND: &str = "detect-collisions";
/// A project's roster bound, shared with the engine and ds-brain.
const MAX_TRANSFORMERS: usize = 2_000;
/// The engine's result and Core's result file; the save route admits 32 MiB.
const MAX_RESULT_BYTES: u64 = 64 * 1024 * 1024;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);
const DETECT_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// How many names a bounded receipt lists before it counts the rest.
const LISTED: usize = 20;

const READ_ARG: Arg = Arg::value(
    "read",
    "<mode>",
    "refresh reads current heads and fetches moved or missing rooms; held makes no request and uses rooms already held.",
)
.choices(&["refresh", "held"]);
const OUT_ARG: Arg = Arg::value(
    "out",
    "<absolute-path>",
    "Write the full result, with its room basis, to a fresh private JSON file for publish.",
);
const RESULT_ARG: Arg = Arg::value(
    "result",
    "<absolute-path>",
    "A private result file `ds design collisions detect --out` wrote.",
)
.required();

macro_rules! refusal {
    ($name:ident, $code:literal, $when:literal, $remedy:literal) => {
        pub const $name: Refusal = Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        };
    };
}

refusal!(
    ENGINE_MISSING,
    "reporter_engine_missing",
    "the selected lane has no managed Reporter addon",
    "install Reporter for the selected --lane; use DS_REPORT_BIN only for explicit development"
);
refusal!(
    ENGINE_RECEIPT_INVALID,
    "module_receipt_invalid",
    "the selected lane's managed Reporter receipt or executable bytes are invalid",
    "repair or roll back Reporter in the selected lane; no alternate engine is substituted"
);
refusal!(
    ENGINE_INCOMPATIBLE,
    "collisions_engine_incompatible",
    "the installed Reporter predates the detect-collisions task",
    "update Reporter for the selected lane"
);
refusal!(
    ENGINE_REFUSED,
    "engine_refused",
    "the Reporter refused the collision request",
    "read detail.engine for the engine's own message"
);
refusal!(
    STATUS_NOT_HELD,
    "design_status_not_held",
    "--read held found no retained transformer status for this account and project",
    "run once with --read refresh while connected"
);
refusal!(
    PROJECT_TOO_LARGE,
    "collisions_project_too_large",
    "the project lists more transformers than one detection admits (2,000)",
    "report the project's size to DS; one detection reads at most 2,000 transformers"
);
refusal!(
    INPUTS_UNAVAILABLE,
    "collisions_inputs_unavailable",
    "no transformer room could be supplied, so no result can replace a previous one",
    "reconnect and run with --read refresh, or hold the rooms first"
);
refusal!(
    SCRATCH_FAILED,
    "collisions_scratch_failed",
    "the private scratch folder for the engine request could not be prepared",
    "check the DS data folder is writable and has free space"
);
refusal!(
    RESULT_INVALID,
    "collisions_result_invalid",
    "the engine result or the result file is malformed, oversized, or answers another request",
    "run ds design collisions detect again and publish its new file"
);
refusal!(
    RESULT_FOREIGN,
    "collisions_result_foreign",
    "the result file belongs to another project, lane or account",
    "detect again for this project, lane and account"
);
refusal!(
    OUTPUT_INVALID,
    "collisions_output_invalid",
    "--out is not a fresh absolute file in an existing private directory",
    "choose a new absolute path in an existing private directory"
);
refusal!(
    BASIS_STALE,
    "collisions_basis_stale",
    "a transformer changed, appeared or left after the result was computed",
    "run ds design collisions detect again, then publish its new result"
);
refusal!(
    PUBLICATION_INVALID,
    "collisions_publication_invalid",
    "ds-brain refused the publication's shape or bounds",
    "publish the unmodified result file ds design collisions detect wrote"
);

const DETECT_REFUSALS: &[Refusal] = &crate::headless_refusals!(
    ENGINE_MISSING,
    ENGINE_RECEIPT_INVALID,
    ENGINE_INCOMPATIBLE,
    ENGINE_REFUSED,
    STATUS_NOT_HELD,
    PROJECT_TOO_LARGE,
    INPUTS_UNAVAILABLE,
    SCRATCH_FAILED,
    RESULT_INVALID,
    RESULT_FOREIGN,
    OUTPUT_INVALID,
);

const PUBLISH_REFUSALS: &[Refusal] = &crate::headless_refusals!(
    crate::NOT_PERMITTED,
    crate::READ_ONLY,
    crate::CONFIRMATION_REQUIRED,
    RESULT_INVALID,
    RESULT_FOREIGN,
    BASIS_STALE,
    PUBLICATION_INVALID,
);

pub static DETECT: Command = Command {
    id: "design.collisions.detect",
    path: &["design", "collisions", "detect"],
    contract: 1,
    summary: "Detect project transformer collisions on this machine.",
    purpose: "\
Runs collision detection locally; nothing is computed in the cloud. Reads the \
project's transformer heads, acquires every active transformer's saved room \
through the governed room hold (current held rooms are reused, moved or \
missing rooms are fetched), and runs the Reporter addon's fixed \
detect-collisions task over the layers its request schema names. --read held \
makes no request: the rooms already on this machine are the input. A room \
that cannot be supplied is named in input_errors, never treated as \
collision-free. Nothing is published; --out writes the private result `ds \
design collisions publish` stores.",
    chapter: Chapter::Design,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, READ_ARG, OUT_ARG, LANE_ARG],
    output: "\
Lane and project, computed_at, the transformer count, `regions` (dissolved \
colliding regions) and `underlying_pairs`, per-layer feature counts, a bounded \
`input_errors` list, the room acquisition receipt (reused, fetched, heads read \
or unread and why) and the engine identity. With --out, `result` is the \
ds.design-collisions-file/v1 receipt: path, sha256, size_bytes.",
    examples: &[
        Example {
            command: "ds design collisions detect --project <id> --output json",
            note: "`.data.regions` is how many colliding regions a publish would store.",
            runnable: false,
        },
        Example {
            command: "ds design collisions detect --project <id> --out /abs/private/collisions.json --output json",
            note: "Then `ds design collisions publish --project <id> --result /abs/private/collisions.json --yes`.",
            runnable: false,
        },
    ],
    refusals: DETECT_REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[
        "detect collisions",
        "overlap",
        "shared customers",
        "crossing lines",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static PUBLISH: Command = Command {
    id: "design.collisions.publish",
    path: &["design", "collisions", "publish"],
    contract: 1,
    summary: "Store one locally detected collisions result for the project.",
    purpose: "\
Publishes the result file `ds design collisions detect --out` wrote. ds-brain \
computes nothing: it admits design.report, refuses a result whose transformer \
basis is no longer current (collisions_basis_stale), and replaces the \
project's stored collisions whole. The file must belong to the same project, \
lane and account.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, RESULT_ARG, LANE_ARG],
    output: "\
Lane and project, `saved`, `regions`, per-layer counts, `chunk_count`, the \
transformer count of the basis and computed_at.",
    examples: &[Example {
        command: "ds design collisions publish --project <id> --result /abs/private/collisions.json --yes",
        note: "A moved transformer refuses with collisions_basis_stale; detect again.",
        runnable: false,
    }],
    refusals: PUBLISH_REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &["save collisions", "publish collisions"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The project-wide documents that are never collision inputs.
pub(crate) fn collision_input(name: &str) -> bool {
    let reserved = matches!(
        name,
        "collisions" | "combined_transformer" | "all_transformers" | "mv_data"
    ) || name
        .strip_prefix("collisions_")
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()));
    !name.is_empty() && !reserved
}

fn head_version(row: &Value) -> Value {
    row.get("metadata")
        .and_then(|metadata| metadata.get("version"))
        .and_then(|version| {
            version
                .as_i64()
                .or_else(|| version.as_f64().map(|n| n as i64))
        })
        .map_or(Value::Null, Value::from)
}

fn rfc3339_now() -> String {
    ds_command_kernel::time::OffsetDateTime::now_utc()
        .format(&ds_command_kernel::time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn scratch_failed(detail: impl Into<String>) -> Failure {
    Failure::failed(SCRATCH_FAILED.code, detail.into()).remedy(SCRATCH_FAILED.remedy)
}

fn output_invalid(detail: impl Into<String>) -> Failure {
    Failure::invalid(OUTPUT_INVALID.code, detail.into()).remedy(OUTPUT_INVALID.remedy)
}

fn result_invalid(detail: impl Into<String>) -> Failure {
    Failure::invalid(RESULT_INVALID.code, detail.into()).remedy(RESULT_INVALID.remedy)
}

fn capture_engine(lane: &str) -> Result<ds_cli_exec::CapturedExternal, Failure> {
    ds_cli_report::DS_REPORT.capture_for_lane_in(lane, &ds_cli_exec::addons::managed_module_base()?)
}

/// The room layers the engine's own published request schema admits. The
/// engine owns its input contract; Core projects rooms by it rather than by a
/// hand copy, and an engine without the task refuses as incompatible.
fn engine_input_layers(engine: &ds_cli_exec::CapturedExternal) -> Result<Vec<String>, Failure> {
    let catalogue = engine.call_json("task-schemas", &[], DISCOVERY_TIMEOUT)?;
    let task = catalogue["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|task| task["subcommand"] == SUBCOMMAND))
        .ok_or_else(|| {
            Failure::unavailable(
                ENGINE_INCOMPATIBLE.code,
                "the installed Reporter does not publish the detect-collisions task",
            )
            .remedy(ENGINE_INCOMPATIBLE.remedy)
        })?;
    let schema = &task["request_schema"];
    let layers: Vec<String> = schema["properties"]["transformers"]["items"]["properties"]["layers"]
        ["properties"]
        .as_object()
        .map(|layers| layers.keys().cloned().collect())
        .unwrap_or_default();
    if schema["properties"]["schema"]["const"] != REQUEST_SCHEMA
        || layers.is_empty()
        || task["network"] != "none"
    {
        return Err(Failure::unavailable(
            ENGINE_INCOMPATIBLE.code,
            "the installed Reporter publishes another detect-collisions contract",
        )
        .remedy(ENGINE_INCOMPATIBLE.remedy));
    }
    Ok(layers)
}

/// A writer that digests exactly the bytes it writes.
struct Digesting<W> {
    inner: W,
    digest: Sha256,
}

impl<W: Write> Write for Digesting<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.digest.update(&buf[..written]);
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// One private scratch folder per run, removed with its request and result.
fn scratch() -> Result<tempfile::TempDir, Failure> {
    let root = ds_layer_store::default_root()
        .map_err(scratch_failed)?
        .join("collisions-scratch");
    ds_layer_store::private::create_dir_all(&root).map_err(|e| scratch_failed(e.to_string()))?;
    tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(&root)
        .map_err(|e| scratch_failed(e.to_string()))
}

fn validate_output(path: &Path) -> Result<(), Failure> {
    if !path.is_absolute()
        || path.to_str().is_none()
        || path.try_exists().unwrap_or(true)
        || !path.parent().is_some_and(Path::is_dir)
    {
        return Err(output_invalid(
            "use a fresh absolute path in an existing private directory",
        ));
    }
    ds_layer_store::snapshot_privacy::directory(path.parent().expect("checked parent"))
        .map_err(output_invalid)
}

fn write_private_new(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let mut file = ds_layer_store::private::open_options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| output_invalid(error.to_string()))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(path);
        return Err(output_invalid(error.to_string()));
    }
    Ok(())
}

fn first_names<'a>(names: impl Iterator<Item = &'a str>) -> Value {
    let names: Vec<&str> = names.collect();
    json!({
        "count": names.len(),
        "first": names.iter().take(LISTED).collect::<Vec<_>>(),
    })
}

pub fn detect(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = super::transformer::named_project(inputs)?;
    let lane = inputs.require("lane")?;
    let refresh = inputs.value("read").unwrap_or("refresh") == "refresh";
    let out = inputs.value("out").map(PathBuf::from);
    if let Some(path) = &out {
        validate_output(path)?;
    }
    // The engine is admitted before any credential or network is touched.
    let engine = capture_engine(lane)?;
    let layers = engine_input_layers(&engine)?;

    let (status, observation) =
        ds_cli_auth::read_transformer_status_for_project(lane, &project, refresh)?;
    let identity = status.identity().clone();
    let rows: Vec<(String, Value)> = status
        .result()
        .rows()
        .iter()
        .filter(|row| collision_input(row.name()))
        .map(|row| (row.name().to_owned(), row.row().clone()))
        .collect();
    if rows.len() > MAX_TRANSFORMERS {
        return Err(Failure::invalid(
            PROJECT_TOO_LARGE.code,
            format!("the project lists more than {MAX_TRANSFORMERS} transformers"),
        )
        .remedy(PROJECT_TOO_LARGE.remedy));
    }
    let names: Vec<String> = rows.iter().map(|(name, _)| name.clone()).collect();
    let heads: Vec<Value> = rows
        .iter()
        .map(|(name, row)| json!({"name": name, "version": head_version(row), "layers": row.get("layers")}))
        .collect();
    // Only a fresh, reachable status read justifies fetching rooms. A held or
    // failed read means the rooms already on this machine are the input.
    let live = refresh && observation["source"] == "refresh" && observation["state"] == "ready";

    let user = if status.user_email().is_empty() {
        identity.uid().to_owned()
    } else {
        status.user_email().to_owned()
    };
    let computed_at = rfc3339_now();
    let scratch = scratch()?;
    let request_path = scratch.path().join("request.json");
    let result_path = scratch.path().join("result.json");
    let scratch_error = |error: std::io::Error| scratch_failed(error.to_string());
    let file = ds_layer_store::private::open_options()
        .write(true)
        .create_new(true)
        .open(&request_path)
        .map_err(scratch_error)?;
    let mut request = Digesting {
        inner: BufWriter::new(file),
        digest: Sha256::new(),
    };
    write!(
        request,
        "{{\"schema\":{},\"project\":{},\"user\":{},\"computed_at\":{},\"transformers\":[",
        json!(REQUEST_SCHEMA),
        json!(project),
        json!(user),
        json!(computed_at)
    )
    .map_err(scratch_error)?;
    let mut basis: Vec<Value> = Vec::new();
    let acquisition = ds_cli_report::project::hold::acquire_transformer_rooms(
        lane,
        &project,
        &identity,
        &names,
        live.then_some(heads.as_slice()),
        |room, _source| {
            let selected: Map<String, Value> = room
                .layers
                .into_iter()
                .filter(|(name, _)| layers.iter().any(|layer| layer == name))
                .collect();
            if !basis.is_empty() {
                request.write_all(b",").map_err(scratch_error)?;
            }
            serde_json::to_writer(
                &mut request,
                &json!({"name": room.transformer, "layers": selected}),
            )
            .map_err(|error| scratch_failed(error.to_string()))?;
            basis.push(json!({"name": room.transformer, "version": room.version}));
            Ok(())
        },
    )?;
    if basis.is_empty() && !names.is_empty() {
        return Err(Failure::unavailable(
            INPUTS_UNAVAILABLE.code,
            format!(
                "none of the project's {} transformer rooms could be supplied{}",
                names.len(),
                acquisition
                    .unreachable
                    .as_deref()
                    .map(|why| format!(" ({why})"))
                    .unwrap_or_default()
            ),
        )
        .remedy(INPUTS_UNAVAILABLE.remedy));
    }
    let input_errors: Vec<Value> = acquisition
        .failures
        .iter()
        .map(|(name, failure)| {
            let message: String = format!("{}: {}", failure.code(), failure.message())
                .chars()
                .take(480)
                .collect();
            json!({"transformer": name, "error_message": message})
        })
        .collect();
    write!(
        request,
        "],\"input_errors\":{}}}",
        Value::Array(input_errors.clone())
    )
    .map_err(scratch_error)?;
    request.flush().map_err(scratch_error)?;
    let request_sha256 = format!("{:x}", request.digest.clone().finalize());
    drop(request);

    let args = [
        OsString::from("--request"),
        request_path.clone().into(),
        OsString::from("--result"),
        result_path.clone().into(),
    ];
    let completed = engine.call(SUBCOMMAND, &args, DETECT_TIMEOUT)?;
    if !completed.succeeded() {
        return Err(ds_cli_report::DS_REPORT.failure_from(&completed, SUBCOMMAND));
    }
    let engine_result = read_bounded_json(&result_path)?;
    if engine_result["schema"] != ENGINE_RESULT_SCHEMA
        || engine_result["status"] != "completed"
        || engine_result["project"] != project.as_str()
        || engine_result["computed_at"] != computed_at.as_str()
        || engine_result["request_sha256"] != request_sha256.as_str()
        || !engine_result["layers"].is_object()
        || engine_result["outcome"]["stage"] != "collision_detection"
    {
        return Err(result_invalid(
            "the engine answered another request or an unknown result shape",
        ));
    }
    drop(scratch);

    let document = json!({
        "schema": RESULT_SCHEMA,
        "lane": status.lane(),
        "project": project,
        "principal": identity.uid(),
        "computed_at": computed_at,
        "engine": engine_result["engine"],
        "request_sha256": request_sha256,
        "basis": basis,
        "input_errors": input_errors,
        "acquisition": acquisition_receipt(&acquisition, &observation, live),
        "inputs": engine_result["inputs"],
        "layers": engine_result["layers"],
        "outcome": engine_result["outcome"],
    });
    let mut output = summary(&document);
    if let Some(path) = out {
        let bytes =
            serde_json::to_vec(&document).map_err(|error| result_invalid(error.to_string()))?;
        write_private_new(&path, &bytes)?;
        if ds_cli_auth::headless_identity_for_named_project(lane)? != identity {
            let _ = std::fs::remove_file(&path);
            return Err(Failure::conflict(
                RESULT_FOREIGN.code,
                "the account changed during detection",
            )
            .remedy(RESULT_FOREIGN.remedy));
        }
        output["result"] = json!({
            "schema": FILE_SCHEMA,
            "path": path,
            "sha256": format!("{:x}", Sha256::digest(&bytes)),
            "size_bytes": bytes.len(),
        });
    }
    Ok(output)
}

fn acquisition_receipt(
    acquisition: &ds_cli_report::project::hold::RoomsAcquisition,
    observation: &Value,
    live: bool,
) -> Value {
    let mut receipt = acquisition.receipt.clone();
    receipt["heads"] = json!(if live { "read" } else { "held" });
    receipt["status_observation"] = observation.clone();
    if let Some(why) = &acquisition.unreachable {
        receipt["unreachable"] = json!(why);
    }
    receipt
}

/// The bounded answer: counts and names, never the geometry.
fn summary(document: &Value) -> Value {
    let layer_counts: Map<String, Value> = document["layers"]
        .as_object()
        .map(|layers| {
            layers
                .iter()
                .map(|(name, layer)| {
                    (
                        name.clone(),
                        json!(layer["features"].as_array().map_or(0, Vec::len)),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let details = &document["outcome"];
    json!({
        "lane": document["lane"],
        "project": {"ds_project": document["project"]},
        "computed_at": document["computed_at"],
        "transformers": document["basis"].as_array().map_or(0, Vec::len),
        "regions": details["regions"],
        "underlying_pairs": details["underlying_pairs"],
        "layers": layer_counts,
        "input_errors": first_names(
            document["input_errors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|error| error["transformer"].as_str()),
        ),
        "acquisition": document["acquisition"],
        "engine": {
            "engine_version": document["engine"]["engine_version"],
            "source_sha": document["engine"]["source_sha"],
        },
        "published": false,
    })
}

fn read_bounded_json(path: &Path) -> Result<Value, Failure> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|error| result_invalid(error.to_string()))?;
    if !metadata.is_file() || metadata.len() > MAX_RESULT_BYTES {
        return Err(result_invalid("the result is not a bounded regular file"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_RESULT_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| result_invalid(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| result_invalid(error.to_string()))
}

/// The ds-brain publication carried by one Core result document.
pub fn publication(document: &Value) -> Result<ds_client_core::CollisionsPublication, Failure> {
    if document["schema"] != RESULT_SCHEMA {
        return Err(result_invalid("not a ds.design-collisions-result/v1 file"));
    }
    serde_json::from_value(json!({
        "schema": PUBLICATION_SCHEMA,
        "computed_at": document["computed_at"],
        "engine": document["engine"],
        "basis": document["basis"],
        "input_errors": document["input_errors"],
        "layers": document["layers"],
        "outcome": document["outcome"],
    }))
    .map_err(|error| result_invalid(error.to_string()))
}

pub fn publish(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = super::transformer::named_project(inputs)?;
    let lane = inputs.require("lane")?;
    let path = PathBuf::from(inputs.require("result")?);
    if !path.is_absolute() {
        return Err(result_invalid("--result must be an absolute path"));
    }
    ds_layer_store::snapshot_privacy::file(&path).map_err(result_invalid)?;
    let document = read_bounded_json(&path)?;
    // The whole file is proved before any credential is read.
    let publication = publication(&document)?;
    publication.validate().map_err(|error| {
        result_invalid(format!(
            "the file is not a publishable collisions result ({error})"
        ))
    })?;
    let foreign = || {
        Failure::conflict(
            RESULT_FOREIGN.code,
            "the result file belongs to another project, lane or account",
        )
        .remedy(RESULT_FOREIGN.remedy)
    };
    if document["project"] != project.as_str() {
        return Err(foreign());
    }
    let identity = ds_cli_auth::headless_identity_for_named_project(lane)?;
    if document["lane"] != identity.lane() || document["principal"] != identity.uid() {
        return Err(foreign());
    }
    let saved = ds_cli_auth::save_collisions(lane, &project, &publication)?;
    let receipt = saved.result();
    Ok(json!({
        "lane": saved.lane(),
        "project": {"ds_project": saved.project_id()},
        "saved": receipt.saved,
        "regions": receipt.regions,
        "layers": receipt.layers,
        "chunk_count": receipt.chunk_count,
        "transformers": receipt.transformers,
        "computed_at": receipt.computed_at,
    }))
}

pub fn render(data: &Value) -> String {
    let mut line = format!(
        "project {} · {} · {} regions · {} transformers",
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["regions"],
        data["transformers"],
    );
    if data["saved"] == true {
        line.push_str(" · saved");
    } else if let Some(path) = data["result"]["path"].as_str() {
        line.push_str(&format!(" · result {path}"));
    }
    if let Some(count) = data["input_errors"]["count"]
        .as_u64()
        .filter(|count| *count > 0)
    {
        line.push_str(&format!(" · {count} rooms unavailable"));
    }
    line.push('\n');
    line
}

#[cfg(test)]
#[path = "collisions_detect_tests.rs"]
mod tests;
