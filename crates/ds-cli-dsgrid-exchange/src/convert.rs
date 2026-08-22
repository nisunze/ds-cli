//! `ds dsgrid-exchange convert` — plan it, then write exactly that plan.
//!
//! This is the only command in the domain that writes. It plans first,
//! refuses a plan carrying any blocker, executes, and materializes the
//! artifacts under `--out` alongside an `exchange-report.json`.
//!
//! Three properties are worth stating because they are the reason this is
//! safe to hand to an agent:
//!
//! * **The plan is re-pinned.** `execute_conversion` re-digests every source
//!   and refuses if the bytes differ from what the plan pinned. A file edited
//!   between `plan` and `convert` fails loudly instead of converting
//!   something nobody previewed.
//! * **The artifact plan owns path identity.** Output paths come from the
//!   plan. This command will not mint a `1-` prefix, flatten a nested
//!   library, or resolve a case-only collision — it refuses, because
//!   materializing bytes at a path the plan did not name means the verified
//!   plan and the written tree are different documents.
//! * **It never overwrites.** An existing output path is a refusal, not a
//!   silent replacement. There is deliberately no `--force`: the remedy is a
//!   new directory, which keeps the previous conversion's evidence intact.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_exchange::conversion::{
    ConversionError, ConversionOutcome, ConversionPlan, execute_conversion, plan_conversion,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{refusals, render, request, sources, token};

/// Artifacts listed in the result. The full count is always reported; a
/// workspace conversion can write thousands of members and the list is not
/// the answer — the digests and the report path are.
const WRITTEN_LIMIT: usize = 25;

pub static COMMAND: Command = Command {
    id: "dsgrid-exchange.convert",
    path: &["dsgrid-exchange", "convert"],
    contract: 1,
    summary: "Execute a conversion and write its artifacts and exchange report.",
    purpose: "\
Plans the conversion, refuses if the plan carries a blocker, then executes it \
and writes every artifact under --out together with an exchange-report.json. \
Source bytes are re-digested against the plan's pins before anything is \
written, so a source that changed since planning stops the run. Existing \
output paths are never overwritten.",
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &ARGS,
    output: "\
The plan id, the outcome status, per-source status with snapshot \
fingerprints, and each written artifact with its sha256 — plus the path of \
the exchange report.",
    examples: &[
        Example {
            command: "ds dsgrid-exchange convert --source ./workspace --target dsgrid --out ./out --output json",
            note: "Import a PLS-CADD workspace into a canonical .dsgrid.",
            runnable: false,
        },
        Example {
            command: "ds dsgrid-exchange convert --source ./model.dsgrid --target kmz --out ./out --output json",
            note: "Export a model for GIS, after reading the losses `plan` reported.",
            runnable: false,
        },
    ],
    refusals: &REFUSALS,
    reference: Some("docs/reference/dsgrid-exchange.md"),
    availability: available,
};

/// `plan`'s inputs plus the output directory. Declared by splicing rather
/// than restating, because "convert takes exactly what plan takes" is the
/// property that makes previewing worth doing.
static ARGS: [Arg; 11] = args();

const fn args() -> [Arg; 11] {
    let mut out = [Arg::switch("", ""); 11];
    let mut index = 0;
    while index < request::SHARED_ARGS.len() {
        out[index] = request::SHARED_ARGS[index];
        index += 1;
    }
    out[index] = Arg::value(
        "out",
        "<dir>",
        "Directory to write artifacts and the exchange report into.",
    )
    .required();
    out
}

static REFUSALS: [Refusal; 12] = refusals::splice(&[
    sources::SHARED_REFUSALS,
    request::REQUEST_REFUSALS,
    CONVERT_REFUSALS,
]);

/// Exactly the codes `run` and its helpers emit — no more. A declared refusal
/// nothing can raise is as misleading as an undeclared one that can: both
/// leave a caller planning for the wrong failures.
const CONVERT_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "plan_blocked",
        when: "the conversion plan carries at least one blocker",
        remedy: "run `ds dsgrid-exchange plan` with the same flags and resolve each blocker",
    },
    Refusal {
        code: "source_digest_mismatch",
        when: "a source's bytes changed between planning and execution",
        remedy: "re-run; the plan pins the bytes it previewed and will not convert different ones",
    },
    Refusal {
        code: "output_path_collision",
        when: "the plan emits the same output path twice",
        remedy: "report this: a plan that names one path twice is an engine defect, not an input error",
    },
    Refusal {
        code: "output_exists",
        when: "an artifact path already exists under --out",
        remedy: "convert into a new directory; this command never overwrites and has no --force",
    },
    Refusal {
        code: "output_unwritable",
        when: "--out cannot be created or written",
        remedy: "check the path and its permissions",
    },
    Refusal {
        code: "report_unserializable",
        when: "the exchange report could not be encoded",
        remedy: "report this: the artifacts were written, but their evidence document was not",
    },
];

fn available() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let out_dir = PathBuf::from(inputs.require("out")?);
    let loaded = sources::load(inputs.repeated("source"))?;
    let request = request::build(inputs, loaded.sources)?;
    let plan = plan_conversion(&request);

    if !plan.blockers.is_empty() {
        return Err(Failure::conflict(
            "plan_blocked",
            "the conversion plan carries blockers and will not run",
        )
        .remedy("resolve each blocker, or convert a source set that does not raise them")
        .next("ds dsgrid-exchange plan --source <path> --target <format>")
        .detail(json!({
            "plan_id": plan.plan_id,
            "blockers": plan.blockers,
        })));
    }

    let outcome = execute_conversion(&plan, &request.sources).map_err(execution_failure)?;
    let written = materialize(&out_dir, &outcome)?;
    let report_path = write_report(&out_dir, &plan, &outcome)?;

    Ok(project(&plan, &outcome, written, &report_path))
}

/// Map the engine's typed refusal onto this command's stable codes.
///
/// `DigestMismatch` and `CandidateCountMismatch` are one situation from the
/// caller's side — the bytes are not the bytes that were previewed — so they
/// share a code, and the detail carries which candidate and which digests.
fn execution_failure(error: ConversionError) -> Failure {
    match error {
        ConversionError::Blocked { count } => Failure::conflict(
            "plan_blocked",
            "the conversion plan carries blockers and will not run",
        )
        .remedy("run `ds dsgrid-exchange plan` and resolve each blocker")
        .detail(json!({ "blockers": count })),
        ConversionError::CandidateCountMismatch { plan, sources } => Failure::conflict(
            "source_digest_mismatch",
            "the source set presented for execution is not the one that was planned",
        )
        .remedy("re-run; the plan pins the bytes it previewed")
        .detail(json!({ "planned_candidates": plan, "given_candidates": sources })),
        ConversionError::DigestMismatch {
            index,
            display_name,
            pinned,
            actual,
        } => Failure::conflict(
            "source_digest_mismatch",
            format!("`{display_name}` changed since the plan pinned it"),
        )
        .remedy("re-run; the plan pins the bytes it previewed")
        .detail(json!({
            "source_index": index,
            "name": display_name,
            "pinned": pinned,
            "actual": actual,
        })),
    }
}

/// Write every output the outcome produced, under `out_dir`.
///
/// Paths come from the plan verbatim. The collision check is a defect
/// detector, not a repair: minting a distinct name would materialize a tree
/// the plan does not describe.
fn materialize(out_dir: &Path, outcome: &ConversionOutcome) -> Result<Vec<Value>, Failure> {
    create_dir(out_dir)?;

    let mut written = Vec::with_capacity(outcome.outputs.len());
    let mut claimed: BTreeSet<String> = BTreeSet::new();

    for output in &outcome.outputs {
        let relative = &output.relative_path;

        // Case-insensitive, because Windows and macOS treat two paths
        // differing only in case as one file. A collision that only appears
        // on the operator's filesystem is a collision this must catch here.
        if !claimed.insert(relative.to_ascii_lowercase()) {
            return Err(Failure::internal(
                "output_path_collision",
                format!("the plan emits `{relative}` more than once"),
            )
            .remedy("report this: a plan naming one path twice is an engine defect")
            .detail(json!({ "path": relative })));
        }

        let target = out_dir.join(relative);
        if target.exists() {
            return Err(Failure::conflict(
                "output_exists",
                format!("`{}` already exists", target.display()),
            )
            .remedy("convert into a new directory; this command never overwrites")
            .detail(json!({ "path": relative })));
        }

        if let Some(parent) = target.parent() {
            create_dir(parent)?;
        }
        write_bytes(&target, &output.bytes)?;

        written.push(json!({
            "path": relative,
            "artifact_type": output.artifact_type,
            "byte_len": output.bytes.len(),
            "sha256": format!("sha256:{}", hex(&output.bytes)),
            "source_index": output.source_index,
        }));
    }

    Ok(written)
}

/// Write the exchange report beside the artifacts.
///
/// The report carries the plan and the per-source outcome together, because
/// the pair is the evidence: the plan says what was promised, the outcome
/// says what happened, and a reader comparing them needs both in one file.
fn write_report(
    out_dir: &Path,
    plan: &ConversionPlan,
    outcome: &ConversionOutcome,
) -> Result<PathBuf, Failure> {
    let path = out_dir.join("exchange-report.json");
    if path.exists() {
        return Err(Failure::conflict(
            "output_exists",
            format!("`{}` already exists", path.display()),
        )
        .remedy("convert into a new directory; this command never overwrites"));
    }

    let document = json!({
        "plan": plan,
        "status": token(&outcome.status),
        "per_source": outcome.per_source,
        "reports": outcome.reports,
    });
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| {
        Failure::internal(
            "report_unserializable",
            "the exchange report did not serialize",
        )
        .detail(json!({ "detail": error.to_string() }))
    })?;
    write_bytes(&path, &bytes)?;
    Ok(path)
}

fn create_dir(path: &Path) -> Result<(), Failure> {
    std::fs::create_dir_all(path).map_err(|error| {
        Failure::failed(
            "output_unwritable",
            format!("cannot create `{}`", path.display()),
        )
        .remedy("check the path and its permissions")
        .detail(json!({ "detail": error.kind().to_string() }))
    })
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    std::fs::write(path, bytes).map_err(|error| {
        Failure::failed(
            "output_unwritable",
            format!("cannot write `{}`", path.display()),
        )
        .remedy("check the path and its permissions")
        .detail(json!({ "detail": error.kind().to_string() }))
    })
}

fn hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn project(
    plan: &ConversionPlan,
    outcome: &ConversionOutcome,
    written: Vec<Value>,
    report_path: &Path,
) -> Value {
    let total = written.len();
    let mut listed = written;
    let withheld = listed.len().saturating_sub(WRITTEN_LIMIT);
    listed.truncate(WRITTEN_LIMIT);

    let per_source: Vec<Value> = outcome
        .per_source
        .iter()
        .map(|status| {
            json!({
                "source_index": status.source_index,
                "name": status.display_name,
                "status": token(&status.status),
                "detail": status.detail,
                "snapshot_fingerprint": status.snapshot_fingerprint,
                "artifact_digests": status.artifact_digests,
            })
        })
        .collect();

    let mut answer = json!({
        "plan_id": plan.plan_id,
        "status": token(&outcome.status),
        "target": token(&plan.target),
        "written": listed,
        "written_count": total,
        "report": report_path.to_string_lossy(),
        "per_source": per_source,
        "warnings": plan.warnings,
        "losses": plan.losses,
    });

    if withheld > 0 {
        answer["more"] = json!({
            "written_withheld": withheld,
            "next": "read the exchange report for every artifact",
        });
    }

    answer
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{}  plan {}\n  {} artifact(s) → {}\n",
        data["status"].as_str().unwrap_or(""),
        data["plan_id"].as_str().unwrap_or(""),
        data["written_count"],
        data["report"].as_str().unwrap_or(""),
    );

    if let Some(rows) = data["written"].as_array().filter(|rows| !rows.is_empty()) {
        out.push('\n');
        for row in rows {
            out.push_str(&format!(
                "  {:<44} {:>10} B  {}\n",
                row["path"].as_str().unwrap_or(""),
                row["byte_len"],
                row["sha256"].as_str().unwrap_or(""),
            ));
        }
    }

    if let Some(withheld) = data["more"]["written_withheld"].as_u64().filter(|n| *n > 0) {
        out.push_str(&format!("  … {withheld} more artifact(s)\n"));
    }

    render::list(&mut out, "LOSSES", &data["losses"]);
    render::list(&mut out, "WARNINGS", &data["warnings"]);
    out
}
