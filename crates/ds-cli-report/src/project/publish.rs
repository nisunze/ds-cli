//! `ds report project publish` — publish report artifacts this machine
//! already holds, without running the engine again.
//!
//! The blocker this exists for: an operator produced a day of reports, and
//! 593 verified artifacts — 379 MB — stayed on one PC where nobody else could
//! see or reuse them. Re-running the engine to rescue them would cost another
//! day and would produce *different* bytes for the same rooms, so the only
//! honest route is to publish the bytes that already exist.
//!
//! What it does NOT do is as important as what it does. It never runs
//! `ds-report`, never fetches a room, and never edits an artifact. It reads
//! each run's own `report-run.json`, **re-hashes every file it declares**, and
//! refuses by name when a digest does not verify — because publishing bytes
//! that are not the bytes the receipt attests to would put a lie in the shared
//! store, which is worse than leaving them stranded.
//!
//! It is idempotent by construction: a run whose `client_run_id` is already
//! committed to the publication queue is reported as already published and
//! nothing is promoted for it. Pointing this command at the same directory
//! twice cannot stack a second copy of the same work on the edge.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::report::PublicationState;
use ds_report_artifacts::{VerifiedSidecarArtifact, confined_fs::HeldDirectory};
use serde_json::{Value, json};

use super::{LANE_ARG, TRANSFORMER_ARG};

const FROM_ARG: Arg = Arg::value(
    "from",
    "<path>",
    "An `ds report project export --out-dir` directory to publish from. Nothing in it is modified.",
)
.required();
const SERVER_STATE_DIR_ARG: Arg = Arg::value(
    "server-state-dir",
    "<absolute-path>",
    "Matching `ds server serve --state-dir` directory when the Server uses a custom state root.",
);

/// One export directory holds one batch per transformer; this bounds a
/// mistaken `--from /` long before it bounds a real report set.
const MAX_RUNS: usize = 2_000;

/// A run receipt is a document, not a payload.
const MAX_RUN_RECEIPT_BYTES: u64 = 4 * 1024 * 1024;

const FROM_INVALID: Refusal = Refusal {
    code: "report_publish_source_invalid",
    when: "--from is not a readable directory holding exported transformer folders",
    remedy: "pass the --out-dir an export wrote; each transformer folder holds report-run.json",
};
const NOTHING_TO_PUBLISH: Refusal = Refusal {
    code: "report_publish_nothing_held",
    when: "--from holds no run receipt for a transformer in scope",
    remedy: "check the path, or drop --transformer to publish every run it holds",
};
const RECEIPT_INVALID: Refusal = Refusal {
    code: "report_publish_receipt_invalid",
    when: "a report-run.json is not this contract's run receipt, or names no artifact",
    remedy: "publish a directory written by this release's `ds report project export`",
};
const FOREIGN_PROJECT: Refusal = Refusal {
    code: "report_publish_foreign_project",
    when: "a run receipt names a different project from the selected one",
    remedy: "select that project with `ds auth project use`, then publish again",
};
const DIGEST_MISMATCH: Refusal = Refusal {
    code: "report_artifact_digest_mismatch",
    when: "an artifact on disk does not match the digest its receipt attests to",
    remedy: "re-run the export for the named transformer; these bytes cannot be published",
};
const ARTIFACT_MISSING: Refusal = Refusal {
    code: "report_artifact_missing",
    when: "a run receipt declares an artifact the directory no longer holds",
    remedy: "re-run the export for the named transformer",
};
const LOCAL_ONLY: Refusal = Refusal {
    code: "report_publish_local_only",
    when: "a held run was produced by a development reporter build, which may not publish",
    remedy: "re-export that transformer with a release reporter build",
};
const SCOPE_CHANGED: Refusal = Refusal {
    code: "report_publish_scope_changed",
    when: "the native identity, lane, audience, project or credential generation changed while sealing",
    remedy: "repeat the publication under the current native account and project",
};
const ROOT_INVALID: Refusal = Refusal {
    code: "report_publish_root_invalid",
    when: "the Server state root is unavailable, relative, or cannot hold a sealed publication",
    remedy: "use Server's default state root, or the same absolute --server-state-dir as ds server serve",
};

const REFUSALS: &[Refusal] = &[
    super::NATIVE_PROFILE,
    super::NATIVE_PROFILE_DIGEST,
    super::NATIVE_PROFILE_UNSAFE,
    super::HEADLESS_SIGNED_OUT,
    super::HEADLESS_NO_PROJECT,
    super::PROJECT_CONTEXT_STALE,
    super::NATIVE_STATE_UNSAFE,
    super::NATIVE_STATE_UNAVAILABLE,
    super::NATIVE_STATE_PROTECTION,
    super::NATIVE_STATE_ROOT,
    super::NATIVE_STATE_CONFLICT,
    super::AUTH_CONTEXT_MISMATCH,
    super::AUTH_REJECTED,
    super::AUTH_REVOKED,
    super::AUTH_IDENTITY_MISMATCH,
    super::AUTH_TRANSIENT,
    super::NOT_FOUND,
    super::INVALID_SCOPE,
    super::RESERVED_IDENTITY,
    super::CONFIRMATION_REQUIRED,
    FROM_INVALID,
    NOTHING_TO_PUBLISH,
    RECEIPT_INVALID,
    FOREIGN_PROJECT,
    DIGEST_MISMATCH,
    ARTIFACT_MISSING,
    LOCAL_ONLY,
    SCOPE_CHANGED,
    ROOT_INVALID,
];

pub static COMMAND: Command = Command {
    id: "report.project.publish",
    path: &["report", "project", "publish"],
    contract: 1,
    summary: "Publish already-exported report artifacts from disk (needs --yes).",
    purpose: "\
Publishes report artifacts this machine already holds, WITHOUT running the \
engine again: it reads each transformer folder's report-run.json, re-hashes \
every artifact it declares, refuses by name if a digest does not verify, and \
seals what verifies into the one publication queue. A run already committed \
to that queue is reported and skipped, so publishing the same directory twice \
changes nothing. This is the route for artifacts an older release wrote \
without publishing them.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[FROM_ARG, TRANSFORMER_ARG, SERVER_STATE_DIR_ARG, LANE_ARG],
    output: "\
Lane, project, the source directory, and one row per run: transformer, \
`state` (`queued`, `already_queued` or `refused`), artifact count, bytes and \
the batch identity it was sealed under. Totals name what entered the queue \
and what was already there.",
    examples: &[
        Example {
            command: "ds report project publish --from ./reports --yes --output json",
            note: "Publishes every run in that export directory; `ds report outbox status` then shows the queue.",
            runnable: false,
        },
        Example {
            command: "ds report project publish --from ./reports --transformer tx_a --yes",
            note: "One room's held artifacts, verified and queued.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["rescue", "stranded", "unpublished", "from disk"],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    ds_cli_auth::native_availability()
}

/// One run receipt, reduced to what a publication needs and nothing else.
#[derive(Debug)]
struct HeldRun {
    project_id: String,
    transformer: String,
    client_run_id: String,
    directory: PathBuf,
    transformer_revision: i64,
    input_base_fingerprint: String,
    room_content_sha256: String,
    engine_version: String,
    engine_build_manifest_sha256: String,
    outputs: Vec<HeldOutput>,
}

#[derive(Debug)]
struct HeldOutput {
    format: String,
    filename: String,
    sha256: String,
    size_bytes: u64,
    paper_size: Option<String>,
    presentation: Option<ds_command_kernel::printing::ArtifactPresentation>,
}

// One constructor per refusal, each naming its own code as a literal path.
// A single generic `invalid(refusal, …)` helper reads better and is worse:
// `refusal_coverage.rs` scans the source for which code a call site can emit,
// and a code behind a parameter is a code nobody can audit.
fn receipt_invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(RECEIPT_INVALID.code, message).remedy(RECEIPT_INVALID.remedy)
}

fn from_invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(FROM_INVALID.code, message).remedy(FROM_INVALID.remedy)
}

fn artifact_missing(message: impl Into<String>) -> Failure {
    Failure::invalid(ARTIFACT_MISSING.code, message).remedy(ARTIFACT_MISSING.remedy)
}

fn digest_mismatch(message: impl Into<String>) -> Failure {
    Failure::invalid(DIGEST_MISMATCH.code, message).remedy(DIGEST_MISMATCH.remedy)
}

fn foreign_project(message: impl Into<String>) -> Failure {
    Failure::invalid(FOREIGN_PROJECT.code, message).remedy(FOREIGN_PROJECT.remedy)
}

fn text(receipt: &Value, field: &str, path: &Path) -> Result<String, Failure> {
    receipt
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| receipt_invalid(format!("{} lacks {field}", path.display())))
}

/// Read one `report-run.json` and accept it only if it is this contract's run
/// receipt. A document that merely looks like one is refused rather than
/// half-read: every field below becomes part of a durable publication.
fn read_run(directory: &Path) -> Result<Option<HeldRun>, Failure> {
    let path = directory.join(ds_command_kernel::report_export::RUN_RECEIPT_FILE);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return Ok(None);
    };
    if !metadata.is_file() || metadata.len() > MAX_RUN_RECEIPT_BYTES {
        return Err(receipt_invalid(format!(
            "{} is not a bounded regular file",
            path.display()
        )));
    }
    let bytes = std::fs::read(&path)
        .map_err(|error| receipt_invalid(format!("{}: {error}", path.display())))?;
    let receipt: Value = serde_json::from_slice(&bytes)
        .map_err(|error| receipt_invalid(format!("{}: {error}", path.display())))?;
    if receipt["schema"] != ds_command_kernel::report_export::RUN_RECEIPT_SCHEMA
        || receipt["task"] != ds_command_kernel::report_export::REPORT_TASK
    {
        return Err(receipt_invalid(format!(
            "{} is not a report run receipt",
            path.display()
        )));
    }
    let transformer_revision = receipt["transformer_revision"]
        .as_i64()
        .filter(|revision| *revision > 0)
        .ok_or_else(|| {
            receipt_invalid(format!(
                "{} lacks a positive transformer revision",
                path.display()
            ))
        })?;
    let mut outputs = Vec::new();
    for output in receipt["outputs"].as_array().into_iter().flatten() {
        outputs.push(HeldOutput {
            format: text(output, "format", &path)?,
            filename: text(output, "filename", &path)?,
            sha256: text(output, "sha256", &path)?,
            size_bytes: output["size_bytes"].as_u64().unwrap_or_default(),
            paper_size: output["paper_size"].as_str().map(str::to_string),
            // A print artifact carries the paper identity it was rendered
            // on. It is re-parsed into its typed form rather than passed
            // through as JSON so a receipt with a malformed presentation is
            // refused here, not by the promotion half.
            presentation: match output.get("presentation") {
                None | Some(Value::Null) => None,
                Some(value) => Some(serde_json::from_value(value.clone()).map_err(|error| {
                    receipt_invalid(format!("{}: presentation — {error}", path.display()))
                })?),
            },
        });
    }
    if outputs.is_empty() {
        return Err(receipt_invalid(format!(
            "{} declares no artifact to publish",
            path.display()
        )));
    }
    Ok(Some(HeldRun {
        // Carried so the caller can refuse a foreign project BY NAME before a
        // single byte is promoted; this reader itself holds no notion of
        // which project is the right one.
        project_id: text(&receipt, "project_id", &path)?,
        transformer: text(&receipt, "transformer", &path)?,
        client_run_id: text(&receipt, "client_run_id", &path)?,
        directory: directory.to_path_buf(),
        transformer_revision,
        input_base_fingerprint: text(&receipt, "input_base_fingerprint", &path)?,
        room_content_sha256: text(&receipt, "room_content_sha256", &path)?,
        engine_version: text(&receipt, "engine_version", &path)?,
        engine_build_manifest_sha256: text(&receipt, "engine_build_manifest_sha256", &path)?,
        outputs,
    }))
}

/// Re-hash what the receipt attests to, and refuse the whole run by name if
/// one file disagrees.
///
/// The promotion path hashes again while it copies, so this is not the only
/// guard — it is the one that produces a refusal an operator can act on, with
/// the file, the expected digest and the digest actually on disk, before any
/// durable state exists.
fn verify(run: &HeldRun) -> Result<(u64, Vec<(usize, String)>), Failure> {
    let mut bytes = 0_u64;
    let mut observed = Vec::with_capacity(run.outputs.len());
    for (index, output) in run.outputs.iter().enumerate() {
        let path = run.directory.join(&output.filename);
        let metadata = std::fs::metadata(&path).map_err(|error| {
            artifact_missing(format!(
                "{} declares {} which cannot be read: {error}",
                run.transformer, output.filename
            ))
        })?;
        if !metadata.is_file() || metadata.len() != output.size_bytes {
            return Err(digest_mismatch(format!(
                "{}: {} is {} bytes; its receipt attests {}",
                run.transformer,
                output.filename,
                metadata.len(),
                output.size_bytes
            )));
        }
        let content = std::fs::read(&path).map_err(|error| {
            artifact_missing(format!(
                "{}: {} — {error}",
                run.transformer, output.filename
            ))
        })?;
        let digest = ds_compute_runtime::digest(&content);
        if digest != output.sha256 {
            return Err(digest_mismatch(format!(
                "{}: {} hashes to {digest}; its receipt attests {}",
                run.transformer, output.filename, output.sha256
            )));
        }
        bytes = bytes.saturating_add(output.size_bytes);
        observed.push((index, digest));
    }
    Ok((bytes, observed))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let requested = super::transformer_set(inputs)?;
    let lane = inputs.require("lane")?;
    let from = PathBuf::from(inputs.require("from")?);
    let source_metadata = std::fs::metadata(&from)
        .map_err(|error| from_invalid(format!("{}: {error}", from.display())))?;
    if !source_metadata.is_dir() {
        return Err(from_invalid(format!(
            "{} is not a directory",
            from.display()
        )));
    }

    // The same identity fence an export publishes under. A rescue publishes
    // durable bytes and therefore proves the account, lane, audience and
    // project exactly as the producing command did.
    let fence = ds_cli_auth::capture_layer_scope_fence(lane)?;
    let inventory = ds_cli_auth::transformer_inventory(lane, &requested)?;
    let project_id = inventory.project_id().to_string();
    verify_scope(lane, &fence, &project_id)?;
    let root = super::export::server_report_artifacts_root(
        lane,
        inputs.value("server-state-dir").map(Path::new),
    )?;

    let mut directories = vec![from.clone()];
    for entry in std::fs::read_dir(&from)
        .map_err(|error| from_invalid(format!("{}: {error}", from.display())))?
    {
        let entry = entry.map_err(|error| from_invalid(format!("{}: {error}", from.display())))?;
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            directories.push(entry.path());
        }
        if directories.len() > MAX_RUNS {
            return Err(from_invalid(format!(
                "{} holds more than {MAX_RUNS} folders",
                from.display()
            )));
        }
    }
    directories.sort();

    let scope: Vec<String> = requested.names().to_vec();
    let mut runs = Vec::new();
    for directory in &directories {
        let Some(run) = read_run(directory)? else {
            continue;
        };
        if run.project_id != project_id {
            return Err(foreign_project(format!(
                "{} was exported for project {}; the selected project is {project_id}",
                directory.display(),
                run.project_id
            )));
        }
        if !scope.is_empty() && !scope.contains(&run.transformer) {
            continue;
        }
        runs.push(run);
    }
    if runs.is_empty() {
        return Err(Failure::conflict(
            NOTHING_TO_PUBLISH.code,
            format!("{} holds no report run to publish", from.display()),
        )
        .remedy(NOTHING_TO_PUBLISH.remedy));
    }

    // Which runs this machine has already committed. Read once, before any
    // promotion, so a second pass over the same directory is a no-op rather
    // than a second copy of the same work stacking on the edge.
    let committed = already_committed(&root, &project_id)?;

    let mut rows = Vec::with_capacity(runs.len());
    let mut queued = 0_usize;
    let mut already = 0_usize;
    let mut queued_bytes = 0_u64;
    for run in &runs {
        if ds_command_kernel::report::publication_state(&run.engine_version, true, false)
            .map_err(receipt_invalid)?
            == PublicationState::LocalOnly
        {
            return Err(Failure::conflict(
                LOCAL_ONLY.code,
                format!(
                    "{} was produced by a development reporter build, which may not publish",
                    run.transformer
                ),
            )
            .remedy(LOCAL_ONLY.remedy));
        }
        if committed.contains(&run.client_run_id) {
            already += 1;
            rows.push(json!({
                "transformer": run.transformer,
                "state": "already_queued",
                "client_run_id": run.client_run_id,
                "artifacts": run.outputs.len(),
                "note": "this exact run is already in the publication queue; nothing was promoted",
            }));
            continue;
        }
        let (bytes, _) = verify(run)?;
        let receipt = seal(run, fence.uid(), &project_id, &root, &|| {
            verify_scope(lane, &fence, &project_id)
        })?;
        queued += 1;
        queued_bytes = queued_bytes.saturating_add(bytes);
        rows.push(json!({
            "transformer": run.transformer,
            "state": ds_command_kernel::report::PublicationStage::Queued.as_str(),
            "client_run_id": run.client_run_id,
            "client_publish_id": receipt.client_publish_id,
            "batch_id": receipt.batch_id,
            "artifacts": run.outputs.len(),
            "bytes": bytes,
        }));
    }

    let mut output = super::project_receipt(&inventory);
    output["from"] = json!(from.display().to_string());
    output["scope"] = json!({
        "mode": if scope.is_empty() { "every_held_run" } else { "explicit" },
        "requested": scope,
    });
    output["publication"] = json!({
        "root": root.display().to_string(),
        "queued": queued,
        "already_queued": already,
        "queued_bytes": queued_bytes,
        "note": "Verified from each run's own receipt and sealed into the one publication queue; the engine did not run.",
    });
    output["runs"] = json!(rows);
    Ok(output)
}

fn verify_scope(
    lane: &str,
    fence: &ds_cli_auth::LayerScopeFence,
    project_id: &str,
) -> Result<(), Failure> {
    ds_cli_auth::verify_layer_scope_fence(lane, fence, fence.uid(), project_id).map_err(|_| {
        Failure::conflict(
            SCOPE_CHANGED.code,
            "the native publication scope changed before held artifacts could be sealed",
        )
        .remedy(SCOPE_CHANGED.remedy)
    })
}

/// The `client_run_id` of every batch already committed for this project.
fn already_committed(
    root: &Path,
    project_id: &str,
) -> Result<std::collections::BTreeSet<String>, Failure> {
    let opened = HeldDirectory::open_absolute(root)
        .map_err(|error| Failure::failed(ROOT_INVALID.code, error).remedy(ROOT_INVALID.remedy))?;
    let Some(held) = opened else {
        return Ok(std::collections::BTreeSet::new());
    };
    Ok(ds_report_artifacts::publication::list_committed(&held)
        .map_err(|error| Failure::failed(ROOT_INVALID.code, error).remedy(ROOT_INVALID.remedy))?
        .into_iter()
        .filter(|receipt| receipt.project_id == project_id)
        .map(|receipt| receipt.client_run_id)
        .collect())
}

/// Promote one held run into the publication queue.
///
/// The same `ds-report-artifacts` door an export uses, called with facts read
/// from the run's own receipt rather than from a live engine result. The guard
/// is re-checked at the one durable visibility boundary, exactly as the
/// producing path does it.
fn seal(
    run: &HeldRun,
    owner_uid: &str,
    project_id: &str,
    root: &Path,
    guard: &dyn Fn() -> Result<(), Failure>,
) -> Result<ds_report_artifacts::publication::PublicationBatchReceipt, Failure> {
    let directory = HeldDirectory::open_absolute(&run.directory)
        .map_err(artifact_missing)?
        .ok_or_else(|| artifact_missing(format!("{} disappeared", run.directory.display())))?;
    let mut held = Vec::with_capacity(run.outputs.len());
    let mut formats = Vec::with_capacity(run.outputs.len());
    for output in &run.outputs {
        let file = directory
            .open_regular_file(OsStr::new(&output.filename), "held report artifact")
            .map_err(|error| artifact_missing(format!("{}: {error}", output.filename)))?;
        formats.push(output.format.as_str());
        held.push(VerifiedSidecarArtifact {
            format: output.format.clone(),
            filename: output.filename.clone(),
            size_bytes: output.size_bytes,
            sha256: output.sha256.clone(),
            paper_size: output.paper_size.clone(),
            presentation: output.presentation.clone(),
            held_file: file,
        });
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let pending = ds_report_artifacts::promote_local_artifacts(
        root,
        &held,
        owner_uid,
        project_id,
        &run.engine_version,
        &run.engine_build_manifest_sha256,
        &run.transformer,
        run.transformer_revision,
        &run.input_base_fingerprint,
        &run.room_content_sha256,
        &run.client_run_id,
        &formats,
        deadline,
    )
    .map_err(|error| Failure::failed(ROOT_INVALID.code, error).remedy(ROOT_INVALID.remedy))?;
    guard()?;
    let committed = pending
        .commit(deadline)
        .map_err(|error| Failure::failed(ROOT_INVALID.code, error).remedy(ROOT_INVALID.remedy))?;
    Ok(committed.receipt)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "project {} ({}) · {} · {} queued · {} already queued\n  from {}\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["publication"]["queued"].as_u64().unwrap_or(0),
        data["publication"]["already_queued"].as_u64().unwrap_or(0),
        data["from"].as_str().unwrap_or("?"),
    );
    for row in data["runs"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<14} {:<28} {} artifacts\n",
            row["state"].as_str().unwrap_or("?"),
            row["transformer"].as_str().unwrap_or("?"),
            row["artifacts"].as_u64().unwrap_or(0),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(transformer: &str, filename: &str, sha256: &str, size: u64) -> Value {
        json!({
            "schema": ds_command_kernel::report_export::RUN_RECEIPT_SCHEMA,
            "task": ds_command_kernel::report_export::REPORT_TASK,
            "project_id": "project-a",
            "transformer": transformer,
            "transformer_revision": 3,
            "client_run_id": format!("report-{}", "0".repeat(32)),
            "input_base_fingerprint": "a".repeat(64),
            "room_content_sha256": "b".repeat(64),
            "engine_version": format!("ds-network-reporter@0.1.0+{}", "c".repeat(40)),
            "engine_build_manifest_sha256": "d".repeat(64),
            "outputs": [{
                "output_id": "xlsx",
                "format": "xlsx",
                "filename": filename,
                "sha256": sha256,
                "size_bytes": size,
            }],
        })
    }

    fn held(directory: &Path, bytes: &[u8], sha256: &str) -> HeldRun {
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join("tx_a.xlsx"), bytes).unwrap();
        std::fs::write(
            directory.join(ds_command_kernel::report_export::RUN_RECEIPT_FILE),
            serde_json::to_vec(&receipt("tx_a", "tx_a.xlsx", sha256, bytes.len() as u64)).unwrap(),
        )
        .unwrap();
        read_run(directory).unwrap().unwrap()
    }

    /// The whole point of publishing from disk: bytes that still match their
    /// receipt go up, and the digest is proven here rather than trusted from a
    /// file written days ago.
    #[test]
    fn a_held_run_whose_bytes_still_match_its_receipt_verifies() {
        let root = tempfile::tempdir().unwrap();
        let digest = ds_compute_runtime::digest(b"x");
        let run = held(&root.path().join("tx_a"), b"x", &digest);
        let (bytes, observed) = verify(&run).unwrap();
        assert_eq!(bytes, 1);
        assert_eq!(observed.len(), 1);
    }

    /// A corrupted or replaced artifact is refused BY NAME before anything
    /// durable exists. Publishing bytes that are not the attested bytes would
    /// put a lie in the shared store, which is worse than leaving them here.
    #[test]
    fn a_bad_digest_is_refused_by_name_and_nothing_is_promoted() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("tx_a");
        let run = held(&directory, b"x", &ds_compute_runtime::digest(b"x"));
        std::fs::write(directory.join("tx_a.xlsx"), b"y").unwrap();
        let failure = verify(&run).unwrap_err();
        assert_eq!(failure.code(), DIGEST_MISMATCH.code);
        assert!(
            failure.message().contains("tx_a.xlsx"),
            "{}",
            failure.message()
        );
        assert!(
            failure
                .message()
                .contains(&ds_compute_runtime::digest(b"x")),
            "the refusal names the attested digest: {}",
            failure.message()
        );
    }

    /// A receipt that declares an artifact the directory no longer holds is a
    /// different failure from a wrong digest, and says so: one is "re-export",
    /// the other is "these bytes are not yours".
    #[test]
    fn a_missing_artifact_is_its_own_refusal_not_a_digest_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("tx_a");
        let run = held(&directory, b"x", &ds_compute_runtime::digest(b"x"));
        std::fs::remove_file(directory.join("tx_a.xlsx")).unwrap();
        assert_eq!(verify(&run).unwrap_err().code(), ARTIFACT_MISSING.code);
    }

    /// A directory with no run receipt is not an error — an export directory's
    /// own root has none — but a document that claims to be one and is not
    /// must never be half-read into a durable publication.
    #[test]
    fn only_this_contracts_run_receipt_is_accepted() {
        let root = tempfile::tempdir().unwrap();
        assert!(read_run(root.path()).unwrap().is_none());
        let directory = root.path().join("tx_b");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(ds_command_kernel::report_export::RUN_RECEIPT_FILE),
            br#"{"schema":"ds.report-run/v0","task":"something_else"}"#,
        )
        .unwrap();
        assert_eq!(
            read_run(&directory).unwrap_err().code(),
            RECEIPT_INVALID.code
        );
    }

    /// Acceptance G is a rescue route, so it has to be discoverable as one and
    /// it must never be able to run the engine.
    #[test]
    fn the_descriptor_publishes_from_disk_and_names_the_digest_refusal() {
        assert_eq!(COMMAND.requires, Requires::Server);
        assert_eq!(COMMAND.effect, Effect::ArtifactWrite);
        assert!(COMMAND.args.iter().any(|arg| arg.name == "from"));
        assert!(COMMAND.purpose.contains("WITHOUT running the"));
        assert!(
            COMMAND
                .refusals
                .iter()
                .any(|refusal| refusal.code == DIGEST_MISMATCH.code)
        );
    }
}
